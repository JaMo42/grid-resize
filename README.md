# grid-resize

Move and resize X11 windows along a grid.

Inspired by [WindowGrid](http://windowgrid.net/) for Windows.

## Usage

The program needs to be actively run the window manager or some other input system as there is no standardized way of only activating during window moving between window managers.

```sh
$ grid-resize <WINDOW> <DIMENSIONS> <CELLS> [OPTIONS]
```

### Arguments

- `WINDOW` either the ID of an X window or `:ACTIVE:` to use the window stored in the `_NET_ACTIVE_WINDOW` on the root window (set by most window managers)

- `DIMENSIONS` the position and size of the grid, given as `x,y,width,height`. For multiple monitors these need to correspond the dimensions provided by Xinerama.

- `CELLS` the number of columns and rows, given as `vertical,horizontal`

### Options

- `--color red,green,blue` the color for the overlay, values are between `0.0` and `1.0`. The default is `0.898,0.513,0.964` (`#EC83E7`).

- `--live` Move and resize the window as the selection changes instead of just at the end.

- `--method METHOD` one of `configure` (default), `message`, or `direct`.

### Methods

The method defines how resizing is done:

- `configure` using the `XConfigureWindow` function, actual resizing is done by the window manager.

- `message` using a client message of type `_NET_MOVERESIZE_WINDOW`, actual resizing is done by the window manager.

- `direct` using the `XMoveResizeWindow` function.

`configure` or `message` are recommenced and may have different results depending on the window manager (like setting the frame size vs setting the client size).
