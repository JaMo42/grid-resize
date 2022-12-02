use super::display::ToXDisplay;
use super::window_builder::WindowBuilder;
use super::*;

#[derive(Copy, Clone)]
pub struct Window {
  handle: XWindow,
  // If this is stored as a pointer the window cannot be sent between threads
  display: usize,
}

impl Window {
  fn display (&self) -> XDisplay {
    self.display as XDisplay
  }

  pub fn from_handle<D: ToXDisplay> (display: &D, handle: XWindow) -> Self {
    Self {
      display: display.to_xdisplay () as usize,
      handle,
    }
  }

  pub fn builder (display: &Display) -> WindowBuilder {
    WindowBuilder::new (display)
  }

  pub fn destroy (&self) {
    unsafe {
      XDestroyWindow (self.display (), self.handle);
    }
  }

  pub fn handle (&self) -> XWindow {
    self.handle
  }

  //pub fn raise (&self) {
  //  unsafe {
  //    XRaiseWindow (self.display (), self.handle);
  //  }
  //}

  pub fn map_raised (&self) {
    unsafe {
      XMapRaised (self.display (), self.handle);
    }
  }

  pub fn move_and_resize (&self, x: i32, y: i32, w: u32, h: u32) {
    unsafe {
      XMoveResizeWindow (self.display (), self.handle, x, y, w, h);
    }
  }

  pub fn send_event (&self, mut event: XEvent, mask: i64) -> bool {
    unsafe {
      XSendEvent (
        self.display (),
        self.handle,
        FALSE,
        mask,
        &mut event as *mut XEvent,
      ) != 0
    }
  }

  pub fn set_class_hint (&self, class: &str, name: &str) {
    unsafe {
      let class_cstr = std::ffi::CString::new (class).unwrap ();
      let name_cstr = std::ffi::CString::new (name).unwrap ();
      let mut h = XClassHint {
        res_class: class_cstr.as_ptr () as *mut i8,
        res_name: name_cstr.as_ptr () as *mut i8,
      };
      XSetClassHint (self.display (), self.handle, &mut h);
    }
  }
}

pub trait ToXWindow {
  fn to_xwindow (&self) -> XWindow;
}

impl ToXWindow for Window {
  fn to_xwindow (&self) -> XWindow {
    self.handle
  }
}

impl ToXWindow for XWindow {
  fn to_xwindow (&self) -> XWindow {
    *self
  }
}

impl PartialEq for Window {
  fn eq (&self, other: &Self) -> bool {
    self.handle == other.handle
  }
}

impl PartialEq<XWindow> for Window {
  fn eq (&self, other: &XWindow) -> bool {
    self.handle == *other
  }
}

impl Eq for Window {}

impl std::fmt::Display for Window {
  fn fmt (&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
    write! (f, "{}", self.handle)
  }
}
