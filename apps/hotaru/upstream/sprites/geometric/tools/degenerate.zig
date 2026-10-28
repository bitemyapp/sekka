const std = @import("std");
const font = @import("src/font/main.zig");
export fn inset_points(w: u32, h: u32, t: u32, out: [*]f64) c_int {
    const alloc=std.heap.c_allocator;
    var canvas=font.sprite.Canvas.init(alloc,w,h,w/4,h/4) catch return -1;
    defer canvas.deinit();
    var path=canvas.staticPath(5);
    path.moveTo(0,0);path.lineTo(@floatFromInt(w),@floatFromInt(h));path.lineTo(@floatFromInt(w),0);path.close();
    var inset=path.wrapped_path.offset(alloc,-@as(f64,@floatFromInt(t))/2) catch return -2;
    defer inset.deinit(alloc);
    var n:usize=0;
    for(inset.nodes.items) |node| {
        const point=switch(node) {.move_to=>|p|p.point,.line_to=>|p|p.point,else=>continue};
        out[n]=point.x;out[n+1]=point.y;n+=2;
    }
    return @intCast(n);
}
