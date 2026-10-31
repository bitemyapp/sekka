// SPDX-License-Identifier: MIT
// Copyright (c) Zig contributors
// Adapted from Zig 0.17.0 lib/std/math/hypot.zig. See LICENSE.zig.
// The explicit fused correction preserves the pinned reference's rounding;
// platform libm hypot may differ by one ULP at an antialias sample boundary.
pub(crate) fn hypot(x: f64, y: f64) -> f64 {
    if x.is_infinite() || y.is_infinite() {
        return f64::INFINITY;
    }
    if x.is_nan() || y.is_nan() {
        return f64::NAN;
    }
    let mut major = x.abs();
    let mut minor = y.abs();
    if minor > major {
        std::mem::swap(&mut major, &mut minor);
    }
    if minor == 0. || major - minor == major {
        return major;
    }
    let lower = f64::MIN_POSITIVE.sqrt();
    let upper = (f64::MAX / 2.).sqrt();
    let scale = f64::from_bits(1) * upper;
    if major > upper {
        fused(major * scale, minor * scale) / scale
    } else if minor < lower {
        fused(major / scale, minor / scale) * scale
    } else {
        fused(major, minor)
    }
}
fn fused(x: f64, y: f64) -> f64 {
    let r = x.mul_add(x, y * y).sqrt();
    let rr = r * r;
    let xx = x * x;
    let z = (-y).mul_add(y, rr - xx) + r.mul_add(r, -rr) - x.mul_add(x, -xx);
    r - z / (2. * r)
}

#[cfg(test)]
mod tests {
    use super::hypot;
    #[test]
    fn pinned_fused_rounding_matches_actual_zig_bit_patterns() {
        // Actual pinned-library observations where platform libm differs.
        for (x, y, expected) in [
            (
                0x402e000000000000_u64,
                0x405c400000000000_u64,
                0x405c7f704671ab73_u64,
            ),
            (
                0x4031000000000000_u64,
                0x403b000000000000_u64,
                0x403fe7f6f939a553_u64,
            ),
            (
                0x4032000000000000_u64,
                0x4065e00000000000_u64,
                0x4065fd8b7fd72888_u64,
            ),
            (
                0x403b000000000000_u64,
                0x4031000000000000_u64,
                0x403fe7f6f939a553_u64,
            ),
            (
                0x403c000000000000_u64,
                0x4047800000000000_u64,
                0x404b5aaa259099ad_u64,
            ),
            (
                0x403e000000000000_u64,
                0x406c400000000000_u64,
                0x406c7f704671ab73_u64,
            ),
            (
                0x4041000000000000_u64,
                0x404b000000000000_u64,
                0x404fe7f6f939a553_u64,
            ),
            (
                0x4041800000000000_u64,
                0x405fc00000000000_u64,
                0x40607781b15b4757_u64,
            ),
            (
                0x4043800000000000_u64,
                0x4069200000000000_u64,
                0x406997f4bf497b77_u64,
            ),
            (
                0x4044800000000000_u64,
                0x4067200000000000_u64,
                0x4067afa4226d9be9_u64,
            ),
            (
                0x4044800000000000_u64,
                0x406c800000000000_u64,
                0x406cf506c059d479_u64,
            ),
            (
                0x4045000000000000_u64,
                0x406a600000000000_u64,
                0x406ae476b2f70c2e_u64,
            ),
            (
                0x4045000000000000_u64,
                0x406d200000000000_u64,
                0x406d982a2bacf839_u64,
            ),
            (
                0x4045800000000000_u64,
                0x4046800000000000_u64,
                0x404f1ee85590e272_u64,
            ),
            (
                0x4045800000000000_u64,
                0x4058800000000000_u64,
                0x405ac132375bf490_u64,
            ),
            (
                0x4045800000000000_u64,
                0x406de00000000000_u64,
                0x406e5acbeed3b55e_u64,
            ),
            (
                0x4046800000000000_u64,
                0x4045800000000000_u64,
                0x404f1ee85590e272_u64,
            ),
            (
                0x4047800000000000_u64,
                0x403c000000000000_u64,
                0x404b5aaa259099ad_u64,
            ),
            (
                0x4048800000000000_u64,
                0x4063800000000000_u64,
                0x40647076fddd2faa_u64,
            ),
            (
                0x4049800000000000_u64,
                0x4061e00000000000_u64,
                0x4062fa4ffda7b819_u64,
            ),
            (
                0x404a800000000000_u64,
                0x4055400000000000_u64,
                0x40590adeeaa19789_u64,
            ),
            (
                0x404b000000000000_u64,
                0x4041000000000000_u64,
                0x404fe7f6f939a553_u64,
            ),
            (
                0x404b000000000000_u64,
                0x4055400000000000_u64,
                0x40592cf649cb3471_u64,
            ),
            (
                0x404b000000000000_u64,
                0x4057400000000000_u64,
                0x405ae29aaa1e7b64_u64,
            ),
            (
                0x404b000000000000_u64,
                0x4065e00000000000_u64,
                0x4066e48b7072e8cd_u64,
            ),
            (
                0x404b800000000000_u64,
                0x4065a00000000000_u64,
                0x4066b10924f79752_u64,
            ),
            (
                0x404b800000000000_u64,
                0x4065c00000000000_u64,
                0x4066cf8a2a4bd01c_u64,
            ),
            (
                0x404b800000000000_u64,
                0x406b400000000000_u64,
                0x406c1a97f14cd199_u64,
            ),
            (
                0x404c000000000000_u64,
                0x4057800000000000_u64,
                0x405b5aaa259099ad_u64,
            ),
            (
                0x404c800000000000_u64,
                0x4052000000000000_u64,
                0x4056f5351ebb1826_u64,
            ),
        ] {
            assert_eq!(
                hypot(f64::from_bits(x), f64::from_bits(y)).to_bits(),
                expected
            );
        }
    }
}
