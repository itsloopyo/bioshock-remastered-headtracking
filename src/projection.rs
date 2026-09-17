#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Matrix(pub [[f32; 4]; 4]);

impl Matrix {
    pub fn multiply(self, other: Self) -> Self {
        let mut result = [[0.0; 4]; 4];
        for (row, output) in result.iter_mut().enumerate() {
            for (column, value) in output.iter_mut().enumerate() {
                *value = (0..4)
                    .map(|i| self.0[row][i] as f64 * other.0[i][column] as f64)
                    .sum::<f64>() as f32;
            }
        }
        Self(result)
    }

    pub fn transform(self, point: [f32; 4]) -> [f32; 4] {
        std::array::from_fn(|column| {
            (0..4)
                .map(|row| point[row] as f64 * self.0[row][column] as f64)
                .sum::<f64>() as f32
        })
    }
}

pub fn weapon_projection(
    clean_view: Matrix,
    clean_inverse_view: Matrix,
    tracked_view: Matrix,
    tracked_inverse_view: Matrix,
    world_projection: Matrix,
    inverse_world_projection: Matrix,
    weapon_projection: Matrix,
) -> Matrix {
    // Preserve the neutral weapon scale, then apply head motion in the world
    // projection. Different FOVs otherwise give head motion different scales.
    tracked_inverse_view
        .multiply(clean_view)
        .multiply(weapon_projection)
        .multiply(inverse_world_projection)
        .multiply(clean_inverse_view)
        .multiply(tracked_view)
        .multiply(world_projection)
}

pub fn project(point: [f32; 4], view_projection: Matrix) -> Option<[f32; 2]> {
    let clip = view_projection.transform(point);
    (clip.iter().all(|value| value.is_finite())
        && clip[3] > 0.0
        && clip[2] >= 0.0
        && clip[2] <= clip[3]
        && clip[0].abs() <= clip[3]
        && clip[1].abs() <= clip[3])
        .then(|| [clip[0] / clip[3], clip[1] / clip[3]])
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDENTITY: Matrix = Matrix([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]);

    fn perspective(scale_x: f32, scale_y: f32) -> (Matrix, Matrix) {
        (
            Matrix([
                [scale_x, 0.0, 0.0, 0.0],
                [0.0, scale_y, 0.0, 0.0],
                [0.0, 0.0, 1.0, 1.0],
                [0.0, 0.0, -1.0, 0.0],
            ]),
            Matrix([
                [1.0 / scale_x, 0.0, 0.0, 0.0],
                [0.0, 1.0 / scale_y, 0.0, 0.0],
                [0.0, 0.0, 0.0, -1.0],
                [0.0, 0.0, 1.0, 1.0],
            ]),
        )
    }

    fn yaw(angle: f32) -> Matrix {
        let (s, c) = angle.to_radians().sin_cos();
        Matrix([
            [c, 0.0, s, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [-s, 0.0, c, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ])
    }

    fn assert_close(a: [f32; 4], b: [f32; 4]) {
        for i in 0..4 {
            assert!((a[i] - b[i]).abs() < 0.0001, "{a:?} != {b:?}");
        }
    }

    fn pitch_roll(pitch: f32, roll: f32) -> Matrix {
        let (sp, cp) = pitch.to_radians().sin_cos();
        let (sr, cr) = roll.to_radians().sin_cos();
        Matrix([
            [1.0, 0.0, 0.0, 0.0],
            [0.0, cp, sp, 0.0],
            [0.0, -sp, cp, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ])
        .multiply(Matrix([
            [cr, sr, 0.0, 0.0],
            [-sr, cr, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]))
    }

    fn inverse_rigid(matrix: Matrix) -> Matrix {
        let mut inverse = IDENTITY;
        for row in 0..3 {
            for column in 0..3 {
                inverse.0[row][column] = matrix.0[column][row];
            }
        }
        let translation = inverse.transform(matrix.0[3]);
        inverse.0[3] = [-translation[0], -translation[1], -translation[2], 1.0];
        inverse
    }

    #[test]
    fn aim_alignment_survives_pitch_roll_and_lean_from_a_rotated_camera() {
        let (world, inverse) = perspective(0.7, 1.4);
        let (weapon, _) = perspective(1.3, 1.8);
        let mut clean = yaw(32.0).multiply(pitch_roll(-14.0, 7.0));
        clean.0[3] = [12.0, -4.0, 8.0, 1.0];
        let clean_inverse = inverse_rigid(clean);
        for pitch in [-30.0, 0.0, 30.0] {
            for roll in [-25.0, 0.0, 25.0] {
                let mut head = yaw(20.0).multiply(pitch_roll(pitch, roll));
                head.0[3] = [0.3, -0.2, 0.1, 1.0];
                let tracked = clean.multiply(head);
                let corrected = weapon_projection(
                    clean,
                    clean_inverse,
                    tracked,
                    inverse_rigid(tracked),
                    world,
                    inverse,
                    weapon,
                );
                let aim = clean_inverse.transform([0.0, 0.0, 10.0, 1.0]);
                assert_close(
                    tracked.multiply(corrected).transform(aim),
                    tracked.multiply(world).transform(aim),
                );
            }
        }
    }

    #[test]
    fn neutral_tracking_preserves_weapon_size() {
        let (world, inverse) = perspective(0.7, 2.5);
        let (weapon, _) = perspective(1.3, 2.5);
        let corrected = weapon_projection(
            IDENTITY, IDENTITY, IDENTITY, IDENTITY, world, inverse, weapon,
        );
        assert_close(
            corrected.transform([0.2, -0.4, 2.0, 1.0]),
            weapon.transform([0.2, -0.4, 2.0, 1.0]),
        );
    }

    #[test]
    fn weapon_and_world_aim_move_together_at_different_fovs() {
        for aspect in [4.0 / 3.0, 16.0 / 9.0, 32.0 / 9.0] {
            let (world, inverse) = perspective(0.7, 0.7 * aspect);
            let (weapon, _) = perspective(1.3, 1.3 * aspect);
            for angle in [-45.0, -20.0, 0.0, 20.0, 45.0] {
                let view = yaw(angle);
                let corrected = weapon_projection(
                    IDENTITY,
                    IDENTITY,
                    view,
                    yaw(-angle),
                    world,
                    inverse,
                    weapon,
                );
                let aim = [0.0, 0.0, 10.0, 1.0];
                assert_close(
                    view.multiply(corrected).transform(aim),
                    view.multiply(world).transform(aim),
                );
            }
        }
    }

    #[test]
    fn points_behind_the_view_are_not_mirrored_onto_the_screen() {
        let (projection, _) = perspective(1.0, 1.0);
        assert_eq!(project([0.0, 0.0, -1.0, 1.0], projection), None);
    }

    #[test]
    fn reticle_rejects_points_inside_the_near_plane_and_outside_the_view() {
        let (projection, _) = perspective(1.0, 1.0);
        for point in [
            [0.0, 0.0, 0.0, 1.0],
            [0.1, 0.1, 0.00001, 1.0],
            [0.0, 0.0, 0.5, 1.0],
            [4.0, 0.0, 2.0, 1.0],
            [0.0, -4.0, 2.0, 1.0],
            [f32::NAN, 0.0, 2.0, 1.0],
            [0.0, 0.0, f32::INFINITY, 1.0],
        ] {
            assert_eq!(project(point, projection), None, "{point:?}");
        }
        assert_eq!(
            project([0.5, -0.5, 2.0, 1.0], projection),
            Some([0.25, -0.25])
        );
    }
}
