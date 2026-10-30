const std = @import("std");
const font = @import("src/font/main.zig");
const box = @import("src/font/sprite/draw/box.zig");
const powerline = @import("src/font/sprite/draw/powerline.zig");
const braille = @import("src/font/sprite/draw/braille.zig");
const geometric = @import("src/font/sprite/draw/geometric_shapes.zig");
const block = @import("src/font/sprite/draw/block.zig");
export fn sprite_raster(cp: u32, w: u32, h: u32, t: u32, output: [*]u8, len: usize) c_int {
    if (w == 0 or h == 0 or w > 4096 or h > 4096 or t == 0 or t > @min(w,h)) return -1;
    if (!((cp >= 0x2500 and cp <= 0x259f) or (cp >= 0x25e2 and cp <= 0x25e5) or (cp >= 0x25f8 and cp <= 0x25fa) or cp==0x25ff or (cp >= 0x2800 and cp <= 0x28ff) or (cp>=0xe0b0 and cp<=0xe0bf) or cp==0xe0d2 or cp==0xe0d4)) return -2;
    const px = w / 4;
    const py = h / 4;
    const needed: usize = @as(usize, w + 2*px) * (h + 2*py);
    if (len != needed) return -3;
    var canvas = font.sprite.Canvas.init(std.heap.c_allocator,w,h,px,py) catch return -4;
    defer canvas.deinit();
    const metrics: font.Metrics = .{.cell_width=w,.cell_height=h,.box_thickness=t};
    if (cp>=0xe000) {
        switch(cp) {
            0xe0b0 => powerline.drawE0B0(cp,&canvas,w,h,metrics) catch return -7,
            0xe0b1 => powerline.drawE0B1(cp,&canvas,w,h,metrics) catch return -7,
            0xe0b2 => powerline.drawE0B2(cp,&canvas,w,h,metrics) catch return -7,
            0xe0b3 => powerline.drawE0B3(cp,&canvas,w,h,metrics) catch return -7,
            0xe0b4 => powerline.drawE0B4(cp,&canvas,w,h,metrics) catch return -7,
            0xe0b5 => powerline.drawE0B5(cp,&canvas,w,h,metrics) catch return -7,
            0xe0b6 => powerline.drawE0B6(cp,&canvas,w,h,metrics) catch return -7,
            0xe0b7 => powerline.drawE0B7(cp,&canvas,w,h,metrics) catch return -7,
            0xe0b8 => powerline.drawE0B8(cp,&canvas,w,h,metrics) catch return -7,
            0xe0b9 => powerline.drawE0B9(cp,&canvas,w,h,metrics) catch return -7,
            0xe0ba => powerline.drawE0BA(cp,&canvas,w,h,metrics) catch return -7,
            0xe0bb => powerline.drawE0BB(cp,&canvas,w,h,metrics) catch return -7,
            0xe0bc => powerline.drawE0BC(cp,&canvas,w,h,metrics) catch return -7,
            0xe0bd => powerline.drawE0BD(cp,&canvas,w,h,metrics) catch return -7,
            0xe0be => powerline.drawE0BE(cp,&canvas,w,h,metrics) catch return -7,
            0xe0bf => powerline.drawE0BF(cp,&canvas,w,h,metrics) catch return -7,
            0xe0d2 => powerline.drawE0D2(cp,&canvas,w,h,metrics) catch return -7,
            0xe0d4 => powerline.drawE0D4(cp,&canvas,w,h,metrics) catch return -7,
            else=>return -2,
        }
    } else if (cp >= 0x2800 and cp <= 0x28ff) braille.draw2800_28FF(cp,&canvas,w,h,metrics) catch return -8
    else if (cp>=0x25e2 and cp<=0x25e5) geometric.draw25E2_25E5(cp,&canvas,w,h,metrics) catch return -9
    else if (cp>=0x25f8 and cp<=0x25fa) geometric.draw25F8_25FA(cp,&canvas,w,h,metrics) catch return -10
    else if (cp==0x25ff) geometric.draw25FF(cp,&canvas,w,h,metrics) catch return -11
    else if (cp <= 0x257f) box.draw2500_257F(cp,&canvas,w,h,metrics) catch return -5
    else block.draw2580_259F(cp,&canvas,w,h,metrics) catch return -6;
    @memcpy(output[0..len],std.mem.sliceAsBytes(canvas.sfc.image_surface_alpha8.buf));
    return 0;
}
