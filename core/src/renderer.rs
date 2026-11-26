use std::mem::ManuallyDrop;
use windows::{
    Win32::{
        Foundation::{BOOL, HWND},
        Graphics::{
            Direct2D::{
                Common::{
                    D2D_POINT_2F, D2D_RECT_F, D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED,
                    D2D1_COLOR_F, D2D1_PIXEL_FORMAT,
                },
                D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_NONE,
                D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1,
                D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_ELLIPSE,
                D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC, D2D1CreateDevice, ID2D1Bitmap1,
                ID2D1Device, ID2D1DeviceContext, ID2D1Image, ID2D1SolidColorBrush,
            },
            Direct3D::{
                D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_10_0, D3D_FEATURE_LEVEL_10_1,
                D3D_FEATURE_LEVEL_11_0,
            },
            Direct3D11::{
                D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice,
                ID3D11Device, ID3D11DeviceContext,
            },
            DirectComposition::{
                DCompositionCreateDevice, IDCompositionDevice, IDCompositionTarget,
                IDCompositionVisual,
            },
            Dxgi::{
                Common::{
                    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_UNKNOWN,
                    DXGI_SAMPLE_DESC,
                },
                CreateDXGIFactory2, DXGI_CREATE_FACTORY_FLAGS, DXGI_PRESENT, DXGI_SCALING_STRETCH,
                DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG, DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIDevice, IDXGIFactory2, IDXGIOutput,
                IDXGISurface, IDXGISwapChain1,
            },
        },
    },
    core::Interface,
};

pub(crate) const CLOSE_BUTTON_RADIUS_DIP: f32 = 50.0;
pub(crate) const IMAGE_MARGIN_DIP: f32 = 80.0;

pub(crate) struct Direct2DRenderer {
    _d3d_device: ID3D11Device,
    _d3d_context: ID3D11DeviceContext,
    _d2d_device: ID2D1Device,
    context: ID2D1DeviceContext,
    swap_chain: IDXGISwapChain1,
    _composition_device: IDCompositionDevice,
    _composition_target: IDCompositionTarget,
    _visual: IDCompositionVisual,
    _target_bitmap: Option<ID2D1Bitmap1>,
    image_bitmap: ID2D1Bitmap1,
    image_width: u32,
    image_height: u32,
    close_button_brush: ID2D1SolidColorBrush,
    foreground_brush: ID2D1SolidColorBrush,
    width: u32,
    height: u32,
    dpi: u32,
}

impl Direct2DRenderer {
    pub(crate) fn new(
        hwnd: HWND,
        width: u32,
        height: u32,
        dpi: u32,
        image: image::RgbaImage,
    ) -> windows::core::Result<Self> {
        let image_width = image.width();
        let image_height = image.height();
        let pitch = image_width.checked_mul(4).ok_or_else(|| {
            windows::core::Error::new(
                windows::core::HRESULT(0x80070057u32 as i32),
                "image row exceeds Direct2D's pitch limit",
            )
        })?;
        let image_pixels = premultiplied_bgra_pixels(image.as_raw());
        let feature_levels = [
            D3D_FEATURE_LEVEL_11_0,
            D3D_FEATURE_LEVEL_10_1,
            D3D_FEATURE_LEVEL_10_0,
        ];
        let mut d3d_device = None;
        let mut d3d_context = None;
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                None,
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&feature_levels),
                D3D11_SDK_VERSION,
                Some(&mut d3d_device),
                None,
                Some(&mut d3d_context),
            )?;
        }
        let d3d_device = d3d_device.ok_or_else(windows::core::Error::from_win32)?;
        let d3d_context = d3d_context.ok_or_else(windows::core::Error::from_win32)?;
        let dxgi_device: IDXGIDevice = d3d_device.cast()?;
        let d2d_device: ID2D1Device = unsafe { D2D1CreateDevice(&dxgi_device, None) }?;
        let context = unsafe { d2d_device.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE) }?;
        let factory: IDXGIFactory2 = unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }?;
        let swap_chain_desc = DXGI_SWAP_CHAIN_DESC1 {
            Width: width,
            Height: height,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            Stereo: BOOL(0),
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            Scaling: DXGI_SCALING_STRETCH,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
            AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
            Flags: 0,
        };
        let swap_chain = unsafe {
            factory.CreateSwapChainForComposition(
                &d3d_device,
                &swap_chain_desc,
                None::<&IDXGIOutput>,
            )
        }?;
        let composition_device: IDCompositionDevice =
            unsafe { DCompositionCreateDevice(&dxgi_device) }?;
        let composition_target = unsafe { composition_device.CreateTargetForHwnd(hwnd, BOOL(1)) }?;
        let visual = unsafe { composition_device.CreateVisual() }?;
        unsafe {
            visual.SetContent(&swap_chain)?;
            composition_target.SetRoot(&visual)?;
            composition_device.Commit()?;
        }

        let target_bitmap = create_target_bitmap(&context, &swap_chain, dpi)?;
        unsafe {
            context.SetTarget(&target_bitmap);
            context.SetDpi(dpi as f32, dpi as f32);
        }
        let image_properties = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            bitmapOptions: D2D1_BITMAP_OPTIONS_NONE,
            colorContext: ManuallyDrop::new(None),
        };
        let image_bitmap = unsafe {
            context.CreateBitmap(
                D2D_SIZE_U {
                    width: image_width,
                    height: image_height,
                },
                Some(image_pixels.as_ptr().cast()),
                pitch,
                &image_properties,
            )
        }?;
        let close_button_brush = unsafe {
            context.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: 0x1d as f32 / 255.0,
                    g: 0x1d as f32 / 255.0,
                    b: 0x1d as f32 / 255.0,
                    a: 1.0,
                },
                None,
            )
        }?;
        let foreground_brush = unsafe {
            context.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: 0.94,
                    g: 0.94,
                    b: 0.94,
                    a: 1.0,
                },
                None,
            )
        }?;

        Ok(Self {
            _d3d_device: d3d_device,
            _d3d_context: d3d_context,
            _d2d_device: d2d_device,
            context,
            swap_chain,
            _composition_device: composition_device,
            _composition_target: composition_target,
            _visual: visual,
            _target_bitmap: Some(target_bitmap),
            image_bitmap,
            image_width,
            image_height,
            close_button_brush,
            foreground_brush,
            width,
            height,
            dpi,
        })
    }

    pub(crate) fn resize(
        &mut self,
        width: u32,
        height: u32,
        dpi: u32,
        close_button_hovered: bool,
    ) -> windows::core::Result<()> {
        if width == 0 || height == 0 {
            return Ok(());
        }
        if self.width != width || self.height != height {
            unsafe {
                self.context.SetTarget(None::<&ID2D1Image>);
            }
            self._target_bitmap = None;
            unsafe {
                self.swap_chain.ResizeBuffers(
                    0,
                    width,
                    height,
                    DXGI_FORMAT_UNKNOWN,
                    DXGI_SWAP_CHAIN_FLAG(0),
                )?;
            }
            let target_bitmap = create_target_bitmap(&self.context, &self.swap_chain, dpi)?;
            unsafe {
                self.context.SetTarget(&target_bitmap);
            }
            self._target_bitmap = Some(target_bitmap);
            self.width = width;
            self.height = height;
        }
        if self.dpi != dpi {
            unsafe {
                self.context.SetDpi(dpi as f32, dpi as f32);
            }
            self.dpi = dpi;
        }
        self.render(close_button_hovered)
    }

    pub(crate) fn render(&self, close_button_hovered: bool) -> windows::core::Result<()> {
        let scale = 96.0 / self.dpi.max(1) as f32;
        let width_dip = self.width as f32 * scale;
        let background_color = if close_button_hovered {
            D2D1_COLOR_F {
                r: 0xcc as f32 / 255.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            }
        } else {
            D2D1_COLOR_F {
                r: 0x1d as f32 / 255.0,
                g: 0x1d as f32 / 255.0,
                b: 0x1d as f32 / 255.0,
                a: 1.0,
            }
        };

        unsafe {
            self.close_button_brush.SetColor(&background_color);
            self.context.BeginDraw();
            self.context.Clear(Some(&D2D1_COLOR_F {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.5,
            }));
            if let Some(destination) = image_destination_rect(
                width_dip,
                self.height as f32 * scale,
                self.image_width,
                self.image_height,
            ) {
                self.context.DrawBitmap(
                    &self.image_bitmap,
                    Some(&destination),
                    1.0,
                    D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC,
                    None,
                    None,
                );
            }
            draw_close_button(
                &self.context,
                &self.close_button_brush,
                &self.foreground_brush,
                width_dip,
            );
            self.context.EndDraw(None, None)?;
            self.swap_chain.Present(1, DXGI_PRESENT(0)).ok()?;
        }

        Ok(())
    }
}

fn create_target_bitmap(
    context: &ID2D1DeviceContext,
    swap_chain: &IDXGISwapChain1,
    dpi: u32,
) -> windows::core::Result<ID2D1Bitmap1> {
    let surface: IDXGISurface = unsafe { swap_chain.GetBuffer(0) }?;
    let properties = D2D1_BITMAP_PROPERTIES1 {
        pixelFormat: D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
        },
        dpiX: dpi as f32,
        dpiY: dpi as f32,
        bitmapOptions: (D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW),
        colorContext: ManuallyDrop::new(None),
    };
    unsafe { context.CreateBitmapFromDxgiSurface(&surface, Some(&properties)) }
}

fn premultiplied_bgra_pixels(rgba: &[u8]) -> Vec<u8> {
    let mut bgra = Vec::with_capacity(rgba.len());
    for &[red, green, blue, alpha] in rgba.as_chunks::<4>().0 {
        let alpha = alpha as u16;
        let premultiply = |channel: u8| ((channel as u16 * alpha + 127) / 255) as u8;
        bgra.extend_from_slice(&[
            premultiply(blue),
            premultiply(green),
            premultiply(red),
            alpha as u8,
        ]);
    }
    bgra
}

fn image_destination_rect(
    window_width_dip: f32,
    window_height_dip: f32,
    image_width: u32,
    image_height: u32,
) -> Option<D2D_RECT_F> {
    if image_width == 0 || image_height == 0 {
        return None;
    }

    let available_width = window_width_dip - IMAGE_MARGIN_DIP * 2.0;
    let available_height = window_height_dip - IMAGE_MARGIN_DIP * 2.0;
    if available_width <= 0.0 || available_height <= 0.0 {
        return None;
    }

    let scale = (available_width / image_width as f32)
        .min(available_height / image_height as f32)
        .min(1.0);
    let width = image_width as f32 * scale;
    let height = image_height as f32 * scale;
    let left = (window_width_dip - width) / 2.0;
    let top = (window_height_dip - height) / 2.0;
    Some(D2D_RECT_F {
        left,
        top,
        right: left + width,
        bottom: top + height,
    })
}

fn draw_close_button(
    context: &ID2D1DeviceContext,
    background_brush: &ID2D1SolidColorBrush,
    foreground_brush: &ID2D1SolidColorBrush,
    width: f32,
) {
    let icon_center = CLOSE_BUTTON_RADIUS_DIP * 0.42;
    unsafe {
        context.FillEllipse(
            &D2D1_ELLIPSE {
                point: D2D_POINT_2F { x: width, y: 0.0 },
                radiusX: CLOSE_BUTTON_RADIUS_DIP,
                radiusY: CLOSE_BUTTON_RADIUS_DIP,
            },
            background_brush,
        );
        context.DrawLine(
            D2D_POINT_2F {
                x: width - icon_center - 5.0,
                y: icon_center - 5.0,
            },
            D2D_POINT_2F {
                x: width - icon_center + 5.0,
                y: icon_center + 5.0,
            },
            foreground_brush,
            1.8,
            None::<&windows::Win32::Graphics::Direct2D::ID2D1StrokeStyle>,
        );
        context.DrawLine(
            D2D_POINT_2F {
                x: width - icon_center + 5.0,
                y: icon_center - 5.0,
            },
            D2D_POINT_2F {
                x: width - icon_center - 5.0,
                y: icon_center + 5.0,
            },
            foreground_brush,
            1.8,
            None::<&windows::Win32::Graphics::Direct2D::ID2D1StrokeStyle>,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_is_downscaled_to_the_margin_bounded_area_and_centered() {
        let destination = image_destination_rect(1000.0, 800.0, 1600, 1200).unwrap();

        let actual = [
            destination.left,
            destination.top,
            destination.right,
            destination.bottom,
        ];
        let expected = [80.0, 85.0, 920.0, 715.0];
        for (actual, expected) in actual.into_iter().zip(expected) {
            assert!((actual - expected).abs() < 0.001);
        }
    }

    #[test]
    fn image_smaller_than_the_available_area_keeps_its_native_size() {
        let destination = image_destination_rect(1000.0, 800.0, 200, 100).unwrap();

        assert_eq!(
            (
                destination.left,
                destination.top,
                destination.right,
                destination.bottom
            ),
            (400.0, 350.0, 600.0, 450.0)
        );
    }

    #[test]
    fn rgba_pixels_are_converted_to_premultiplied_bgra() {
        assert_eq!(
            premultiplied_bgra_pixels(&[255, 128, 64, 128, 11, 22, 33, 0]),
            [32, 64, 128, 128, 0, 0, 0, 0]
        );
    }
}
