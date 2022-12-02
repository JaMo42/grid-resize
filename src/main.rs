#[macro_use]
extern crate simple_error;

use std::{
  ffi::{c_char, c_uchar, c_void, CStr},
  str::FromStr,
};

use cairo::{Context, Operator, Surface};
use cairo_sys::cairo_xlib_surface_create;
use clap::Parser;
use x11::xlib::*;

mod x;
use x::{Display, Window, XDisplay, XWindow};

type StdResult<T, E> = std::result::Result<T, E>;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Copy, Clone)]
enum MoveResizeMethod {
  Direct,
  Configure,
  Message,
}

impl MoveResizeMethod {
  fn from_str (s: &str) -> Result<Self> {
    match s.to_lowercase ().as_str () {
      "direct" => Ok (Self::Direct),
      "configure" => Ok (Self::Configure),
      "message" => Ok (Self::Message),
      _ => bail! ("Invalid method"),
    }
  }
}

#[derive(Parser)]
struct Args {
  /// X window ID, or :ACTIVE: to use the window specified in the
  /// _NET_ACTIVE_WINDOW property
  window: String,
  /// "x,y,width,height"
  dimensions: String,
  /// "vertical,horizontal"
  cells: String,
  /// "red,green,blue", components are between 0.0 and 1.0
  #[arg(long, default_value_t = {"0.898,0.513,0.964".to_string ()})]
  color: String,
  /// Move and resize window as selection changes
  #[arg(long)]
  live: bool,
  /// "configure" to use a configure request, "message" to use a
  /// _NET_MOVERESIZE_WINDOW client message, or "direct" to directly resize the
  /// X window.
  #[arg(long, default_value_t = {"configure".to_string ()})]
  method: String,
}

impl Args {
  fn parse_list<T> (list: &str) -> StdResult<Vec<T>, T::Err>
  where
    T: FromStr,
    T::Err: std::fmt::Debug,
  {
    list.split (',').map (|elem| elem.parse ()).collect ()
  }
}

struct RGB {
  red: f64,
  green: f64,
  blue: f64,
}

impl RGB {
  fn new (red: f64, green: f64, blue: f64) -> Self {
    Self { red, green, blue }
  }

  fn lerp (&self, other: RGB, w: f64) -> RGB {
    Self::new (
      self.red + w * (other.red - self.red),
      self.green + w * (other.green - self.green),
      self.blue + w * (other.blue - self.blue),
    )
  }
}

impl FromStr for RGB {
  type Err = <f64 as FromStr>::Err;

  fn from_str (s: &str) -> StdResult<Self, Self::Err> {
    let list = Args::parse_list (s)?;
    Ok (Self {
      red: list[0],
      green: list[1],
      blue: list[2],
    })
  }
}

unsafe extern "C" fn error_handler (display: XDisplay, event: *mut XErrorEvent) -> i32 {
  const ERROR_TEXT_SIZE: usize = 1024;
  let mut error_text_buf: [c_char; ERROR_TEXT_SIZE] = [0; ERROR_TEXT_SIZE];
  let error_text = &mut error_text_buf as *mut c_char;
  XGetErrorText (
    display,
    (*event).error_code as i32,
    error_text,
    ERROR_TEXT_SIZE as i32,
  );
  let error_msg = CStr::from_ptr (error_text).to_str ().unwrap ().to_owned ();
  eprintln! ("X Error: {}", error_msg);
  0
}

fn get_active_window (display: &Display) -> Result<XWindow> {
  let prop = display.intern_atom ("_NET_ACTIVE_WINDOW");
  let mut _actual_type: Atom = 0;
  let mut _format: i32 = 0;
  let mut _nitems: u64 = 0;
  let mut _bytes_after: u64 = 0;
  let mut data: *mut c_uchar = std::ptr::null_mut ();
  let window;
  unsafe {
    if XGetWindowProperty (
      display.as_raw (),
      display.root (),
      prop,
      0,
      2,
      0,
      XA_WINDOW,
      &mut _actual_type,
      &mut _format,
      &mut _nitems,
      &mut _bytes_after,
      &mut data,
    ) != Success as i32
      || data.is_null ()
    {
      bail! ("No active window");
    }
    window = *(data as *mut XWindow);
    XFree (data as *mut c_void);
  }
  Ok (window)
}

struct Grid {
  vertical_cells: u32,
  horizontal_cells: u32,
  cell_width: u32,
  cell_height: u32,
}

impl Grid {
  fn new (width: u32, height: u32, vertical_cells: u32, horizontal_cells: u32) -> Self {
    Self {
      vertical_cells,
      horizontal_cells,
      cell_width: width / vertical_cells,
      cell_height: height / horizontal_cells,
    }
  }

  /// Returns the top-left corner of the cell containing the given point.
  fn lower_bound (&self, x: i32, y: i32) -> (u32, u32) {
    let mut x_index = 0;
    let mut y_index = 0;
    for i in 0..=self.vertical_cells {
      if i * self.cell_width > x as u32 {
        break;
      }
      x_index = i;
    }
    for i in 0..=self.horizontal_cells {
      if i * self.cell_height > y as u32 {
        break;
      }
      y_index = i;
    }
    (x_index, y_index)
  }

  /// Returns the bottom-rught corner of the cell containing the given point.
  fn upper_bound (&self, x: i32, y: i32) -> (u32, u32) {
    let mut x_index = 0;
    let mut y_index = 0;
    for i in 0..=self.vertical_cells {
      if i * self.cell_width > x as u32 {
        x_index = i;
        break;
      }
    }
    for i in 0..=self.horizontal_cells {
      if i * self.cell_height > y as u32 {
        y_index = i;
        break;
      }
    }
    (x_index, y_index)
  }

  fn position (&self, index: (u32, u32)) -> (u32, u32) {
    (index.0 * self.cell_width, index.1 * self.cell_height)
  }
}

struct Selection {
  p1_x: i32,
  p1_y: i32,
  p2_x: i32,
  p2_y: i32,
}

impl Selection {
  fn new (x: i32, y: i32) -> Self {
    Self {
      p1_x: x,
      p1_y: y,
      p2_x: x,
      p2_y: y,
    }
  }

  fn get (&self, grid: &Grid) -> ((u32, u32), (u32, u32)) {
    // Sort points
    let p1_x = i32::min (self.p1_x, self.p2_x);
    let p2_x = i32::max (self.p1_x, self.p2_x);
    let p1_y = i32::min (self.p1_y, self.p2_y);
    let p2_y = i32::max (self.p1_y, self.p2_y);
    (grid.lower_bound (p1_x, p1_y), grid.upper_bound (p2_x, p2_y))
  }

  fn get_dimensions (&self, grid: &Grid) -> (i32, i32, u32, u32) {
    let (p1idx, p2idx) = self.get (grid);
    let p1 = grid.position (p1idx);
    let p2 = grid.position (p2idx);
    (p1.0 as i32, p1.1 as i32, p2.0 - p1.0, p2.1 - p1.1)
  }
}

/// Correct the given dimensions to account for integer division in the grid
/// logic.
fn correct_dimensions (
  x: i32,
  y: i32,
  width: u32,
  height: u32,
  vertical_cells: u32,
  horizontal_cells: u32,
) -> (i32, i32, u32, u32) {
  let cell_width = width / vertical_cells;
  let cell_height = height / horizontal_cells;
  let use_width = cell_width * vertical_cells;
  let use_height = cell_height * horizontal_cells;
  let use_x = x + (width - use_width) as i32 / 2;
  let use_y = y + (height - use_height) as i32 / 2;
  (use_x, use_y, use_width, use_height)
}

struct GridReize {
  display: Display,
  window: Window,
  gc: GC,
  surface: Surface,
  context: Context,
  x: i32,
  y: i32,
  width: u32,
  height: u32,
  target: Window,
  grid: Grid,
  selection: Selection,
  left_button_held: bool,
  running: bool,
  color: RGB,
  last_box: ((u32, u32), (u32, u32)),
  live: bool,
  last_motion: Time,
  method: MoveResizeMethod,
}

impl GridReize {
  fn new (display: Display, args: &Args) -> Result<Self> {
    let mut dim_iter = args
      .dimensions
      .split (',')
      .map (|d| d.parse::<i64> ().unwrap ());
    if dim_iter.clone ().count () != 4 {
      bail! ("Invalid dimensions, should be: `x,y,width,height`");
    }
    let x = dim_iter.next ().unwrap () as i32;
    let y = dim_iter.next ().unwrap () as i32;
    let width = dim_iter.next ().unwrap () as u32;
    let height = dim_iter.next ().unwrap () as u32;

    let mut cells_iter = args.cells.split (',').map (|c| c.parse::<u32> ().unwrap ());
    if cells_iter.clone ().count () != 2 {
      bail! ("Invalid grid size, should be: `vertical,horizontal`");
    }
    let vertical_cells = cells_iter.next ().unwrap ();
    let horizontal_cells = cells_iter.next ().unwrap ();

    let (x, y, width, height) =
      correct_dimensions (x, y, width, height, vertical_cells, horizontal_cells);

    let vi = display
      .match_visual_info (32, TrueColor)
      .ok_or ("Failed to get RGBA visual")?;
    let colormap = display.create_colormap (vi.visual, AllocNone);

    let window = Window::builder (&display)
      .position (x, y)
      .size (width, height)
      .attributes (|attributes| {
        attributes
          .override_redirect (true)
          .background_pixel (0)
          .border_pixel (0)
          .event_mask (ButtonPressMask | ButtonReleaseMask | PointerMotionMask | KeyPressMask)
          .colormap (colormap)
          .save_under (true);
      })
      .depth (vi.depth)
      .visual (vi.visual)
      .build ();
    window.set_class_hint ("Grid_resize", "grid_resize");
    unsafe {
      let desktop_type = display.intern_atom ("_NET_WM_WINDOW_TYPE_DESKTOP");
      XChangeProperty (
        display.as_raw (),
        window.handle (),
        display.intern_atom ("_NET_WM_WINDOW_TYPE"),
        XA_ATOM,
        32,
        PropModeReplace,
        &desktop_type as *const u64 as *const c_uchar,
        1,
      );
    }

    let gc = unsafe {
      XCreateGC (
        display.as_raw (),
        window.handle (),
        0,
        std::ptr::null_mut (),
      )
    };

    let surface = unsafe {
      let raw = cairo_xlib_surface_create (
        display.as_raw (),
        window.handle (),
        vi.visual,
        width as i32,
        height as i32,
      );
      Surface::from_raw_full (raw)?
    };

    let context = Context::new (&surface)?;
    context.set_operator (Operator::Source);
    context.set_line_width (3.0);

    let target = Window::from_handle (
      &display,
      if args.window == ":ACTIVE:" {
        get_active_window (&display)?
      } else {
        args.window.parse ()?
      },
    );

    let (mouse_x, mouse_y) = display
      .query_pointer_position ()
      .ok_or ("Failed to get pointer position")?;

    Ok (Self {
      display,
      window,
      gc,
      surface,
      context,
      x,
      y,
      width,
      height,
      target,
      grid: Grid::new (width, height, vertical_cells, horizontal_cells),
      selection: Selection::new (mouse_x - x, mouse_y - y),
      left_button_held: false,
      running: false,
      color: RGB::from_str (&args.color)?,
      last_box: ((0, 0), (0, 0)),
      live: args.live,
      last_motion: 0,
      method: MoveResizeMethod::from_str (&args.method)?,
    })
  }

  fn run (&mut self) -> Result<()> {
    self.window.map_raised ();
    self.redraw ()?;
    let mut event: XEvent = unsafe { std::mem::zeroed () };
    self.running = true;
    while self.running {
      self.display.next_event (&mut event);
      #[allow(non_upper_case_globals)]
      match unsafe { event.type_ } {
        ButtonPress => self.button_press (unsafe { &event.button }),
        ButtonRelease => self.button_release (unsafe { &event.button }),
        MotionNotify => self.motion (unsafe { &event.motion }),
        KeyPress => self.key_press (unsafe { &event.key }),
        _ => {}
      }
      let box_ = self.selection.get (&self.grid);
      if box_ != self.last_box {
        self.redraw ()?;
        self.last_box = box_;
        if self.live {
          self.move_and_resize ();
        }
      }
    }
    self.window.destroy ();
    unsafe {
      XFreeGC (self.display.as_raw (), self.gc);
    }
    self.display.set_input_focus (self.target);
    self.display.close ();
    Ok (())
  }

  fn redraw (&mut self) -> Result<()> {
    // Clear
    self.context.set_source_rgba (0.0, 0.0, 0.0, 0.0);
    self.context.paint ()?;
    // Pending area
    {
      self
        .context
        .set_source_rgba (self.color.red, self.color.green, self.color.blue, 0.3);
      let (x, y, w, h) = self.selection.get_dimensions (&self.grid);
      self
        .context
        .rectangle (x as f64, y as f64, w as f64, h as f64);
      self.context.fill ()?;
    }
    // Cell under mouse
    if let Some ((x, y)) = self.display.query_pointer_position () {
      let (x, y) = self.grid.lower_bound (x - self.x, y - self.y);
      let color = RGB::lerp (&self.color, RGB::new (0.9, 0.9, 0.9), 0.5);
      self
        .context
        .set_source_rgba (color.red, color.green, color.blue, 0.5);
      self.context.rectangle (
        (x * self.grid.cell_width) as f64,
        (y * self.grid.cell_height) as f64,
        self.grid.cell_width as f64,
        self.grid.cell_height as f64,
      );
      self.context.fill ()?;
    }
    // Lines
    {
      self
        .context
        .set_source_rgba (self.color.red, self.color.green, self.color.blue, 0.9);
      for i in 0..=self.grid.vertical_cells {
        let x = (i * self.grid.cell_width) as f64;
        self.context.move_to (x, 0.0);
        self.context.line_to (x, self.height as f64);
        self.context.stroke ()?;
      }
      for i in 0..=self.grid.horizontal_cells {
        let y = (i * self.grid.cell_height) as f64;
        self.context.move_to (0.0, y);
        self.context.line_to (self.width as f64, y);
        self.context.stroke ()?;
      }
    }
    self.surface.flush ();
    self.display.flush ();
    Ok (())
  }

  fn button_press (&mut self, event: &XButtonEvent) {
    if event.button == Button3 {
      self.left_button_held = true;
      self.selection.p1_x = event.x;
      self.selection.p1_y = event.y;
    }
  }

  fn button_release (&mut self, event: &XButtonEvent) {
    #[allow(non_upper_case_globals)]
    match event.button {
      Button1 => {
        self.finish ();
      }
      Button3 => {
        self.left_button_held = false;
      }
      _ => {}
    }
  }

  fn motion (&mut self, event: &XMotionEvent) {
    if event.time - self.last_motion < 1000 / 30 {
      return;
    }
    self.last_motion = event.time;
    self.selection.p2_x = event.x;
    self.selection.p2_y = event.y;
    if self.left_button_held {
      self.selection.p1_x = event.x;
      self.selection.p1_y = event.y;
    }
  }

  fn key_press (&mut self, event: &XKeyEvent) {
    if x::lookup_keysym (event) as u32 == x11::keysym::XK_Escape {
      self.cancel ();
    }
  }

  fn cancel (&mut self) {
    self.running = false;
  }

  fn finish (&mut self) {
    self.running = false;
    // If it's in live mode the window was already resized in the mainloop.
    if !self.live {
      self.move_and_resize ();
    }
  }

  fn move_and_resize (&self) {
    let (x, y, w, h) = self.selection.get_dimensions (&self.grid);
    println! ("Resize: {}x{}+{}+{}", w, h, self.x + x, self.y + y);
    match self.method {
      MoveResizeMethod::Direct => {
        self.target.move_and_resize (self.x + x, self.y + y, w, h);
      }
      MoveResizeMethod::Message => {
        // https://specifications.freedesktop.org/wm-spec/wm-spec-1.3.html#idm46463187598320
        let event = XEvent {
          client_message: XClientMessageEvent {
            type_: ClientMessage,
            serial: 0,        // set by XSendEvent
            send_event: True, // set by XSendEvent
            display: self.display.as_raw (),
            window: self.target.handle (),
            message_type: self.display.intern_atom ("_NET_MOVERESIZE_WINDOW"),
            format: 32,
            data: ClientMessageData::from ([
              // From the spec:
              // "The bits 8 to 11 indicate the presence of x, y, width and height"
              // "The bits 12 to 15 indicate the source [...], so 0001 indicates the
              //  application and 0010 indicates a Pager or a Taskbar."
              (NorthWestGravity | (0b1111 << 7) | (0b0010 << 11)) as i64,
              (self.x + x) as i64,
              (self.y + y) as i64,
              w as i64,
              h as i64,
            ]),
          },
        };
        Window::from_handle (&self.display, self.display.root ())
          .send_event (event, SubstructureRedirectMask | SubstructureNotifyMask);
      }
      MoveResizeMethod::Configure => unsafe {
        let mut values: XWindowChanges = std::mem::zeroed ();
        values.x = self.x + x;
        values.y = self.y + y;
        values.width = w as i32;
        values.height = h as i32;
        XConfigureWindow (
          self.display.as_raw (),
          self.target.handle (),
          (CWX | CWY | CWWidth | CWHeight) as u32,
          &mut values,
        );
      },
    }
    self.display.sync (true);
  }
}

fn main () -> Result<()> {
  let args = Args::parse ();
  let display = Display::connect (None);
  x::set_error_handler (error_handler);
  GridReize::new (display, &args)?.run ()
}
