use log::{debug, info, warn};
use nalgebra::Vector2;
use rogue_macros::Resource;
use winit::{
    self,
    dpi::{LogicalSize, PhysicalSize},
    event_loop,
    window::{Window as WinitWindow, WindowAttributes},
};

pub struct WindowConfig {
    pub title: String,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            title: "Pyrite App".to_string(),
        }
    }
}

impl WindowConfig {
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }
}

#[cfg(target_arch = "wasm32")]
pub type WindowHandle = std::rc::Rc<WinitWindow>;
#[cfg(not(target_arch = "wasm32"))]
pub type WindowHandle = std::sync::Arc<WinitWindow>;

#[derive(Resource)]
pub struct Window {
    winit_window: WindowHandle,
    is_first_frame: bool,
    cursor_locked: bool,
}

impl raw_window_handle::HasDisplayHandle for Window {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        self.winit_window.display_handle()
    }
}

impl raw_window_handle::HasWindowHandle for Window {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        self.winit_window.window_handle()
    }
}

impl Window {
    pub fn new(event_loop: &winit::event_loop::ActiveEventLoop) -> Self {
        let mut window_attrs = WindowAttributes::default()
            .with_title("Rogue")
            .with_resizable(true);
        cfg_if::cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                use wasm_bindgen::JsCast;
                use winit::platform::web::WindowAttributesExtWebSys;

                window_attrs = web_sys::window()
                    .and_then(|window| window.document())
                    .and_then(|document| {
                        let canvas = document.get_element_by_id("canvas_target")?.dyn_into::<web_sys::HtmlCanvasElement>().ok()?;

                        Some(window_attrs.with_inner_size(winit::dpi::LogicalSize::new(canvas.width(), canvas.height())).with_canvas(Some(canvas)))
                    })
                    .expect("Couldn't append canvas");
            }
        }

        let winit_window = event_loop
            .create_window(window_attrs)
            .expect("Failed to create window");

        Self {
            winit_window: WindowHandle::new(winit_window),
            is_first_frame: true,
            cursor_locked: false,
        }
    }

    pub fn finish_frame(&mut self) {
        self.is_first_frame = false;
    }

    pub fn is_first_frame(&self) -> bool {
        self.is_first_frame
    }

    pub fn is_cursor_locked(&self) -> bool {
        self.cursor_locked
    }

    pub fn inner_size_vec2(&self) -> Vector2<u32> {
        Vector2::new(self.width(), self.height())
    }

    pub fn set_cursor_grabbed(&self, grabbed: bool) {
        if grabbed {
            if let Err(_) = self
                .winit_window
                .set_cursor_grab(winit::window::CursorGrabMode::Locked)
            {
                if let Err(_) = self
                    .winit_window
                    .set_cursor_grab(winit::window::CursorGrabMode::Confined)
                {
                    warn!("This platform does not support cursor grabbing.");
                }
            }
        } else {
            self.winit_window
                .set_cursor_grab(winit::window::CursorGrabMode::None);
        }
    }

    pub fn set_curser_lock(&mut self, locked: bool) {
        self.set_cursor_visible(!locked);
        self.set_cursor_grabbed(locked);
        self.cursor_locked = locked;
    }

    pub fn set_cursor_visible(&self, visible: bool) {
        self.winit_window.set_cursor_visible(visible);
    }

    pub fn set_visible(&self, visible: bool) {
        self.winit_window.set_visible(visible);
    }

    pub fn width(&self) -> u32 {
        self.winit_window.inner_size().width
    }

    pub fn height(&self) -> u32 {
        self.winit_window.inner_size().height
    }

    pub fn inner_size(&self) -> winit::dpi::PhysicalSize<u32> {
        self.winit_window.inner_size()
    }

    pub fn is_maximized(&self) -> bool {
        self.winit_window.is_maximized()
    }

    pub fn is_minimized(&self) -> bool {
        self.winit_window.is_minimized().unwrap_or(false)
    }

    pub fn handle(&self) -> &WindowHandle {
        &self.winit_window
    }
}
