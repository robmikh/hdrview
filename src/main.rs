#![windows_subsystem = "windows"]

mod composition;
mod d3d;
mod dispatcher_queue;
mod handle;
mod wic;
mod window;

use composition::{create_composition_graphics_device, draw_to_surface};
use d3d::{create_d3d_device, create_texture_from_bytes};
use dispatcher_queue::{
    create_dispatcher_queue_controller_for_current_thread,
    shutdown_dispatcher_queue_controller_and_wait,
};
use wic::{create_wic_factory, load_image_from_decoder};
use window::Window;
use windows::{
    core::{w, Result, HSTRING},
    Foundation::Numerics::Vector2,
    Graphics::{
        DirectX::{DirectXAlphaMode, DirectXPixelFormat},
        SizeInt32,
    },
    Win32::{
        Graphics::{
            Direct3D11::ID3D11Texture2D,
            Dxgi::Common::DXGI_FORMAT_R16G16B16A16_FLOAT,
            Imaging::{
                GUID_WICPixelFormat64bppRGBAHalf, IWICBitmapDecoder, IWICImagingFactory,
                WICDecodeMetadataCacheOnDemand,
            },
        },
        System::{
            Com::STGM_READ,
            WinRT::{RoInitialize, RO_INIT_SINGLETHREADED},
        },
        UI::{
            Shell::{SHCreateMemStream, SHCreateStreamOnFileW},
            WindowsAndMessaging::{
                DispatchMessageW, GetMessageW, MessageBoxW, TranslateMessage, MB_ICONERROR, MB_OK,
                MSG,
            },
        },
    },
    UI::{
        Color,
        Composition::{CompositionStretch, Compositor},
    },
};

const DEFAULT_IMAGE_BYTES: &[u8] = include_bytes!("../assets/hdr-image.jxr");

fn run() -> Result<()> {
    unsafe { RoInitialize(RO_INIT_SINGLETHREADED)? };
    let controller = create_dispatcher_queue_controller_for_current_thread()?;

    // Init D3D11
    let d3d_device = create_d3d_device()?;
    let d3d_context = unsafe { d3d_device.GetImmediateContext()? };

    // Init Composition
    let compositor = Compositor::new()?;
    let root = compositor.CreateSpriteVisual()?;
    root.SetRelativeSizeAdjustment(Vector2::new(1.0, 1.0))?;
    root.SetBrush(&compositor.CreateColorBrushWithColor(Color {
        A: 255,
        R: 0,
        G: 0,
        B: 0,
    })?)?;

    // Create our window and hook up our visual tree
    let window = Window::new("hdrview", 800, 600)?;
    let target = window.create_window_target(&compositor, false)?;
    target.SetRoot(&root)?;

    // Create a CompositionGraphicsDevice for our surface
    let comp_graphics = create_composition_graphics_device(&compositor, &d3d_device)?;

    // Init WIC
    let wic_factory = create_wic_factory()?;
    let decoder = create_wic_decoder_from_args(&wic_factory)?;
    let image =
        load_image_from_decoder(&wic_factory, &decoder, &GUID_WICPixelFormat64bppRGBAHalf, 8)?;
    let width = image.width;
    let height = image.height;
    let stride = image.stride;
    let bytes = image.bytes;

    // Create a texture we'll use to copy to the surface
    let texture = create_texture_from_bytes(
        &d3d_device,
        width,
        height,
        DXGI_FORMAT_R16G16B16A16_FLOAT,
        stride,
        &bytes,
    )?;

    // Create our surface and copy our texture to it
    let surface = comp_graphics.CreateDrawingSurface2(
        SizeInt32 {
            Width: width as i32,
            Height: height as i32,
        },
        DirectXPixelFormat::R16G16B16A16Float,
        DirectXAlphaMode::Premultiplied,
    )?;
    draw_to_surface::<ID3D11Texture2D, _>(
        &surface,
        None,
        |surface_texture, point| -> Result<()> {
            unsafe {
                d3d_context.CopySubresourceRegion(
                    surface_texture,
                    0,
                    point.x as u32,
                    point.y as u32,
                    0,
                    &texture,
                    0,
                    None,
                );
            }
            Ok(())
        },
    )?;

    // Hookup our surface into the visual tree
    let content = compositor.CreateSpriteVisual()?;
    content.SetRelativeSizeAdjustment(Vector2::one())?;
    let brush = compositor.CreateSurfaceBrushWithSurface(&surface)?;
    brush.SetStretch(CompositionStretch::Uniform)?;
    brush.SetHorizontalAlignmentRatio(0.5)?;
    brush.SetVerticalAlignmentRatio(0.5)?;
    content.SetBrush(&brush)?;
    root.Children()?.InsertAtTop(&content)?;

    let mut message = MSG::default();
    unsafe {
        while GetMessageW(&mut message, None, 0, 0).into() {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    // TODO: Do something with the error code.
    let _ = shutdown_dispatcher_queue_controller_and_wait(&controller, message.wParam.0 as i32)?;

    Ok(())
}

fn main() {
    let result = run();

    if let Err(error) = result {
        let message = format!("{:#?}", error);
        let message = HSTRING::from(&message);
        unsafe {
            MessageBoxW(None, &message, w!("hdrview"), MB_OK | MB_ICONERROR);
        }
        std::process::exit(1);
    }
}

fn create_wic_decoder_from_args(wic_factory: &IWICImagingFactory) -> Result<IWICBitmapDecoder> {
    // First check to see if we were passed a path
    let args: Vec<_> = std::env::args().skip(1).collect();
    let stream = if let Some(path) = args.get(0) {
        if !validate_jxr_path(path) {
            unsafe {
                MessageBoxW(
                    None,
                    w!("Expected a JXR file!"),
                    w!("hdrview"),
                    MB_OK | MB_ICONERROR,
                );
            }
            panic!("Expected JXR file!");
        }
        let path = HSTRING::from(path);
        unsafe { SHCreateStreamOnFileW(&path, STGM_READ.0)? }
    } else {
        unsafe { SHCreateMemStream(Some(DEFAULT_IMAGE_BYTES)).unwrap() }
    };

    let decoder = unsafe {
        wic_factory.CreateDecoderFromStream(
            &stream,
            std::ptr::null(),
            WICDecodeMetadataCacheOnDemand,
        )?
    };
    Ok(decoder)
}

fn validate_jxr_path(path: &str) -> bool {
    let path = std::path::PathBuf::from(path);
    if let Some(extension) = path.extension() {
        if let Some(extension) = extension.to_str() {
            let extension = extension.to_lowercase();
            if extension == "jxr" {
                return true;
            }
        }
    }
    false
}
