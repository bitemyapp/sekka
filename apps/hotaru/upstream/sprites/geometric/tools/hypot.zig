const std = @import("std");
export fn reference_hypot(x: f64, y: f64) f64 {
    return std.math.hypot(x,y);
}
