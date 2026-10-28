const std = @import("std");
const font = @import("src/font/main.zig");
const box = @import("src/font/sprite/draw/box.zig");
const block = @import("src/font/sprite/draw/block.zig");
export fn sprite_raster(cp: u32, w: u32, h: u32, t: u32, output: [*]u8, len: usize) c_int {
    if (w == 0 or h == 0 or w > 4096 or h > 4096 or t == 0 or t > @min(w,h)) return -1;
    if (!((cp >= 0x2500 and cp <= 0x259f))) return -2;
    const px = w / 4;
    const py = h / 4;
    const needed: usize = @as(usize, w + 2*px) * (h + 2*py);
    if (len != needed) return -3;
    var canvas = font.sprite.Canvas.init(std.heap.c_allocator,w,h,px,py) catch return -4;
    defer canvas.deinit();
    const metrics: font.Metrics = .{.cell_width=w,.cell_height=h,.box_thickness=t};
    if (cp <= 0x257f) box.draw2500_257F(cp,&canvas,w,h,metrics) catch return -5
    else block.draw2580_259F(cp,&canvas,w,h,metrics) catch return -6;
    @memcpy(output[0..len],std.mem.sliceAsBytes(canvas.sfc.image_surface_alpha8.buf));
    return 0;
}
