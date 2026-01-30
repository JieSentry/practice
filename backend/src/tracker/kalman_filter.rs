use nalgebra::{ArrayStorage, Matrix, Matrix4, U1, U4, U8, Vector4};

type Matrix8<T> = Matrix<T, U8, U8, ArrayStorage<T, 8, 8>>;
type Vector8<T> = Matrix<T, U8, U1, ArrayStorage<T, 8, 1>>;
type Matrix4x8<T> = Matrix<T, U4, U8, ArrayStorage<T, 4, 8>>;

/// A [Kalman Filter] implementation by GPT-5.
///
/// [Kalman Filter]: https://github.com/ultralytics/ultralytics/blob/004d9730060e560c86ad79aaa1ab97167443be25/ultralytics/trackers/utils/kalman_filter.py
#[derive(Debug, Clone)]
pub struct KalmanXYAH {
    pub(super) mean: Vector8<f32>,
    covariance: Matrix8<f32>,
    motion_mat: Matrix8<f32>,
    update_mat: Matrix4x8<f32>,
    std_weight_pos: f32,
    std_weight_vel: f32,
}

impl KalmanXYAH {
    pub fn new() -> Self {
        let mut motion = Matrix8::<f32>::identity();
        for i in 0..4 {
            motion[(i, i + 4)] = 1.0;
        }

        let mut update = Matrix4x8::<f32>::zeros();
        for i in 0..4 {
            update[(i, i)] = 1.0;
        }

        Self {
            mean: Vector8::zeros(),
            covariance: Matrix8::identity(),
            motion_mat: motion,
            update_mat: update,
            std_weight_pos: 1.0 / 20.0,
            std_weight_vel: 1.0 / 160.0,
        }
    }

    pub fn initiate(&mut self, measurement: Vector4<f32>) {
        self.mean = Vector8::zeros();
        self.mean.fixed_rows_mut::<4>(0).copy_from(&measurement);

        let h = measurement[3];

        let std = [
            2.0 * self.std_weight_pos * h,
            2.0 * self.std_weight_pos * h,
            1e-2,
            2.0 * self.std_weight_pos * h,
            10.0 * self.std_weight_vel * h,
            10.0 * self.std_weight_vel * h,
            1e-5,
            10.0 * self.std_weight_vel * h,
        ];

        self.covariance = Matrix8::zeros();
        for (i, std) in std.iter().enumerate() {
            self.covariance[(i, i)] = std * std;
        }
    }

    pub fn predict(&mut self) {
        let h = self.mean[3];
        let std = [
            self.std_weight_pos * h,
            self.std_weight_pos * h,
            1e-2,
            self.std_weight_pos * h,
            self.std_weight_vel * h,
            self.std_weight_vel * h,
            1e-5,
            self.std_weight_vel * h,
        ];

        let mut motion_cov = Matrix8::zeros();
        for i in 0..8 {
            motion_cov[(i, i)] = std[i] * std[i];
        }

        self.mean = self.motion_mat * self.mean;
        self.covariance =
            self.motion_mat * self.covariance * self.motion_mat.transpose() + motion_cov;
    }

    pub fn update(&mut self, measurement: Vector4<f32>) {
        let (projected_mean, projected_cov) = self.project();
        let chol = projected_cov
            .cholesky()
            .expect("Projected covariance not SPD");
        let ph_t = self.covariance * self.update_mat.transpose();
        let kalman_gain = ph_t * chol.solve(&Matrix4::identity());
        let innovation = measurement - projected_mean;

        self.mean += kalman_gain * innovation;
        self.covariance -= kalman_gain * projected_cov * kalman_gain.transpose();
    }

    fn project(&self) -> (Vector4<f32>, Matrix4<f32>) {
        let h = self.mean[3];
        let r_std = [
            self.std_weight_pos * h,
            self.std_weight_pos * h,
            1e-1,
            self.std_weight_pos * h,
        ];

        let mut r = Matrix4::<f32>::zeros();
        for i in 0..4 {
            r[(i, i)] = r_std[i] * r_std[i];
        }

        let mean = self.update_mat * self.mean;
        let cov = self.update_mat * self.covariance * self.update_mat.transpose() + r;
        (mean, cov)
    }

    pub fn gate(&self, measurement: Vector4<f32>, position_only: bool) -> bool {
        let (projected_mean, projected_cov) = self.project();
        let diff = measurement - projected_mean;

        let gate = if position_only {
            let cov_xy = projected_cov.fixed_view::<2, 2>(0, 0).into_owned();
            let diff_xy = diff.fixed_rows::<2>(0).into_owned();

            let chol = cov_xy.cholesky().expect("SPD");
            let z = chol.solve(&diff_xy);
            z.dot(&z)
        } else {
            let chol = projected_cov.cholesky().expect("SPD");
            let z = chol.solve(&diff);
            z.dot(&z)
        };
        let threshold = if position_only { 5.9915 } else { 9.4877 };

        gate > threshold
    }

    pub fn tlwh(&self) -> [f32; 4] {
        let cx = self.mean[0];
        let cy = self.mean[1];
        let a = self.mean[2];
        let h = self.mean[3];

        let w = a * h;
        let x = cx - w / 2.0;
        let y = cy - h / 2.0;

        [x, y, w, h]
    }
}
