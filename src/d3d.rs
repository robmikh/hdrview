use windows::core::{ComInterface, Interface, Result};
use windows::Win32::Graphics::Direct3D11::{ID3D11Texture2D, D3D11_CREATE_DEVICE_DEBUG};
use windows::Win32::Graphics::Dxgi::IDXGISurface;
use windows::Win32::Graphics::{
    Direct3D::{D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP},
    Direct3D11::{
        D3D11CreateDevice, ID3D11Device, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
        D3D11_CREATE_DEVICE_FLAG, D3D11_SDK_VERSION,
    },
    Dxgi::{IDXGIDevice, DXGI_ERROR_UNSUPPORTED},
};

fn create_d3d_device_with_type(
    driver_type: D3D_DRIVER_TYPE,
    flags: D3D11_CREATE_DEVICE_FLAG,
    device: *mut Option<ID3D11Device>,
) -> Result<()> {
    unsafe {
        D3D11CreateDevice(
            None,
            driver_type,
            None,
            flags,
            None,
            D3D11_SDK_VERSION,
            Some(device),
            None,
            None,
        )
    }
}

pub fn create_d3d_device() -> Result<ID3D11Device> {
    let mut device = None;
    let flags = {
        let mut flags = D3D11_CREATE_DEVICE_BGRA_SUPPORT;
        if cfg!(feature = "d3ddebug") {
            flags |= D3D11_CREATE_DEVICE_DEBUG;
        }
        flags
    };
    let mut result = create_d3d_device_with_type(D3D_DRIVER_TYPE_HARDWARE, flags, &mut device);
    if let Err(error) = &result {
        if error.code() == DXGI_ERROR_UNSUPPORTED {
            result = create_d3d_device_with_type(D3D_DRIVER_TYPE_WARP, flags, &mut device);
        }
    }
    result?;
    Ok(device.unwrap())
}

