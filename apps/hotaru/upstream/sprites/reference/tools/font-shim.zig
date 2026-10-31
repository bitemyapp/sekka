// Oracle-only shim. Raster geometry uses these three exact physical fields.
pub const Metrics = struct { cell_width: u32, cell_height: u32, box_thickness: u32 };
pub const sprite = struct {
    pub const Canvas = @import("sprite/canvas.zig").Canvas;
    pub const Color = @import("sprite/canvas.zig").Color;
};
