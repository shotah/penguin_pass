//! What the planner needs to know about the road.

use pp_course::Course;

/// A road the planner can sample by distance.
///
/// `distance_m` is meters along the centerline. Implementations used with a
/// closed loop should accept any finite distance; [`crate::ChunkPlanner`]
/// wraps those distances into `[0, length)` before calling. Open and endless
/// routes are not wrapped. The planner clamps their distances at 0, so
/// `position_at` is not asked for a negative distance.
pub trait Route {
    /// Centerline position at `distance_m`, in meters, as `[x, y, z]`.
    ///
    /// `y` is up. The planner uses `x` and `z` only.
    fn position_at(&self, distance_m: f64) -> [f32; 3];

    /// Length of a closed loop, in meters.
    ///
    /// `Some(len)` wraps. `len` must be finite and greater than zero.
    /// `None` is an open or endless route: distance does not wrap and may
    /// grow without a bound.
    fn length_m(&self) -> Option<f64>;
}

impl Route for Course {
    fn position_at(&self, distance_m: f64) -> [f32; 3] {
        Course::position_at(self, distance_m)
    }

    fn length_m(&self) -> Option<f64> {
        Some(Course::length_m(self))
    }
}

/// An endless straight road along +X at elevation 0 and z = 0.
///
/// Distances below 0, and non-finite distances, sample the origin. There is
/// no end and no wrap. This is the test double for an open route; a generated
/// endless road implements [`Route`] the same way, with [`Route::length_m`]
/// returning [`None`].
#[derive(Debug, Clone, Copy, Default)]
pub struct StraightRoute;

impl Route for StraightRoute {
    fn position_at(&self, distance_m: f64) -> [f32; 3] {
        let distance = if distance_m.is_finite() {
            distance_m.max(0.0)
        } else {
            0.0
        };
        [distance as f32, 0.0, 0.0]
    }

    fn length_m(&self) -> Option<f64> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn course_is_a_closed_loop() {
        let course = Course::penguin_pass();
        let length = Route::length_m(&course).expect("penguin pass wraps");
        assert!((length - 5_000.0).abs() < 1e-3);
        assert_eq!(
            Route::position_at(&course, 0.0),
            Route::position_at(&course, length)
        );
        assert_eq!(
            Route::position_at(&course, -10.0),
            Route::position_at(&course, length - 10.0)
        );
    }

    #[test]
    fn straight_route_is_endless_and_clamps() {
        let route = StraightRoute;
        assert_eq!(route.length_m(), None);
        assert_eq!(route.position_at(0.0), [0.0, 0.0, 0.0]);
        assert_eq!(route.position_at(-50.0), [0.0, 0.0, 0.0]);
        assert_eq!(route.position_at(12.5), [12.5, 0.0, 0.0]);
        assert_eq!(route.position_at(f64::NAN), [0.0, 0.0, 0.0]);
        assert_eq!(route.position_at(f64::INFINITY), [0.0, 0.0, 0.0]);
    }
}
