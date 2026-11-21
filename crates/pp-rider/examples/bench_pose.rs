//! Time `RiderModel::build` and `RiderModel::pose`.
//!
//! Run it in release mode; a debug build is many times slower.
//!
//! ```text
//! cargo run --release -p pp-rider --example bench_pose
//! ```

use std::hint::black_box;
use std::time::Instant;

use pp_rider::{PartId, PoseInput, RiderModel, RiderStyle};

/// Number of poses to time in each mode.
const CALLS: u32 = 2_000_000;

fn main() {
    let start = Instant::now();
    let model = RiderModel::build(&RiderStyle::default());
    let build = start.elapsed();
    println!(
        "build: {:.2} ms for {} triangles in {} parts",
        build.as_secs_f64() * 1000.0,
        model.triangle_count(),
        model.parts().len()
    );

    let modes = [
        ("seated", 0.0, 0.0),
        ("standing", 1.0, 0.0),
        ("tucked", 0.0, 1.0),
    ];
    for (name, stand, tuck) in modes {
        let mut checksum = 0.0_f32;
        let start = Instant::now();
        for i in 0..CALLS {
            let t = i as f32 * 1.0e-3;
            let input = PoseInput {
                crank_angle_rad: t,
                wheel_angle_rad: 3.0 * t,
                steer_rad: 0.1 * (0.01 * t).sin(),
                effort: 0.6,
                stand,
                tuck,
            };
            let pose = model.pose(black_box(&input));
            checksum += pose.transform(PartId::FootR).translation[1];
        }
        let per_call = start.elapsed().as_secs_f64() / f64::from(CALLS);
        println!(
            "pose, {name}: {:.0} ns per call (checksum {checksum})",
            per_call * 1.0e9
        );
    }
}
