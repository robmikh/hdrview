use windows::{
    core::{ComInterface, Result},
    Win32::{
        Foundation::{POINT, RECT},
        Graphics::Direct3D11::ID3D11Device,
        System::WinRT::Composition::{ICompositionDrawingSurfaceInterop, ICompositorInterop},
    },
    UI::Composition::{CompositionDrawingSurface, CompositionGraphicsDevice, Compositor},
};

pub fn create_composition_graphics_device(
    compositor: &Compositor,
    d3d_device: &ID3D11Device,
) -> Result<CompositionGraphicsDevice> {
    let interop: ICompositorInterop = compositor.cast()?;
    let comp_graphics = unsafe { interop.CreateGraphicsDevice(d3d_device)? };
    Ok(comp_graphics)
}

pub fn draw_to_surface<T: ComInterface, F: FnOnce(&T, POINT) -> Result<()>>(
    surface: &CompositionDrawingSurface,
    update_rect: Option<RECT>,
    func: F,
) -> Result<()> {
    let interop: ICompositionDrawingSurfaceInterop = surface.cast()?;
    unsafe {
        let mut point = POINT::default();
        let update_object: T =
            interop.BeginDraw(update_rect.as_ref().map(|x| x as *const _), &mut point)?;
        let result = func(&update_object, point);
        interop.EndDraw()?;
        result
    }
}
