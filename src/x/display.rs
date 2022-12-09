use super::{window::ToXWindow, *};
use std::ffi::CString;

pub struct Display {
  connection: XDisplay,
  screen: c_int,
  root: XWindow,
}

impl Display {
  pub fn connect (name: Option<&str>) -> Self {
    let connection;
    let root;
    let screen;

    unsafe {
      connection = XOpenDisplay (
        name
          .map (|s| s.as_ptr () as *const c_char)
          .unwrap_or (std::ptr::null ()),
      );
      if connection.is_null () {
        let name = name
          .map (|s| s.to_string ())
          .or_else (|| std::env::var ("DISPLAY").ok ())
          .unwrap_or_default ();
        panic! ("Could not open display: {}", name);
      }
      root = XDefaultRootWindow (connection);
      screen = XDefaultScreen (connection);
    }

    Self {
      connection,
      screen,
      root,
    }
  }

  pub fn root (&self) -> XWindow {
    self.root
  }

  pub fn close (&mut self) {
    if !self.connection.is_null () {
      unsafe {
        XCloseDisplay (self.connection);
      }
      self.connection = std::ptr::null_mut ();
    }
  }

  pub fn as_raw (&self) -> XDisplay {
    self.connection
  }

  pub fn flush (&self) {
    unsafe {
      XFlush (self.connection);
    }
  }

  pub fn sync (&self, discard_events: bool) {
    unsafe {
      XSync (self.connection, discard_events as i32);
    }
  }

  pub fn next_event (&self, event_out: &mut XEvent) {
    unsafe {
      XNextEvent (self.connection, event_out);
    }
  }

  pub fn set_input_focus<W: ToXWindow> (&self, window: W) {
    unsafe {
      XSetInputFocus (
        self.connection,
        window.to_xwindow (),
        RevertToParent,
        CurrentTime,
      );
    }
  }

  pub fn query_pointer_position (&self) -> Option<(i32, i32)> {
    let mut x: c_int = 0;
    let mut y: c_int = 0;
    // Dummy values
    let mut i: c_int = 0;
    let mut u: c_uint = 0;
    let mut w: XWindow = NONE;
    if unsafe {
      XQueryPointer (
        self.connection,
        self.root,
        &mut w,
        &mut w,
        &mut x,
        &mut y,
        &mut i,
        &mut i,
        &mut u,
      )
    } == TRUE
    {
      Some ((x, y))
    } else {
      None
    }
  }

  pub fn intern_atom (&self, name: &str) -> Atom {
    unsafe {
      let cstr = CString::new (name).unwrap ();
      XInternAtom (self.connection, cstr.as_ptr (), FALSE)
    }
  }

  pub fn match_visual_info (&self, depth: i32, class: i32) -> Option<XVisualInfo> {
    unsafe {
      let mut vi: XVisualInfo = std::mem::MaybeUninit::zeroed ().assume_init ();
      if XMatchVisualInfo (self.connection, self.screen, depth, class, &mut vi) != 0 {
        Some (vi)
      } else {
        None
      }
    }
  }

  pub fn create_colormap (&self, visual: *mut Visual, alloc: i32) -> Colormap {
    unsafe { XCreateColormap (self.connection, self.root, visual, alloc) }
  }
}

pub trait ToXDisplay {
  fn to_xdisplay (&self) -> XDisplay;
}

impl ToXDisplay for Display {
  fn to_xdisplay (&self) -> XDisplay {
    self.as_raw ()
  }
}

impl ToXDisplay for XDisplay {
  fn to_xdisplay (&self) -> XDisplay {
    *self
  }
}

pub struct ScopedKeyboardGrab {
  connection: XDisplay,
}

impl ScopedKeyboardGrab {
  pub fn grab (display: &Display, window: &Window) -> Self {
    unsafe {
      XGrabKeyboard (
        display.as_raw (),
        window.handle (),
        False,
        GrabModeAsync,
        GrabModeAsync,
        CurrentTime,
      );
    }
    Self {
      connection: display.as_raw (),
    }
  }
}

impl Drop for ScopedKeyboardGrab {
  fn drop (&mut self) {
    unsafe {
      XUngrabKeyboard (self.connection, CurrentTime);
    }
  }
}
