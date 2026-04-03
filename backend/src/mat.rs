use anyhow::Result;
use opencv::{
    boxed_ref::BoxedRef,
    core::{Mat, Vec4b},
};
use platforms::capture::Frame;

/// A BGRA [`Mat`] that owns the external buffer.
#[derive(Debug)]
pub struct OwnedMat {
    rows: i32,
    cols: i32,
    bytes: Vec<u8>,

    #[cfg(debug_assertions)]
    inner: Option<Mat>,
}

impl OwnedMat {
    #[inline]
    pub fn new(frame: Frame) -> Result<Self> {
        let owned = Self {
            rows: frame.height,
            cols: frame.width,
            bytes: frame.data,
            #[cfg(debug_assertions)]
            inner: None,
        };
        let _ = owned.as_mat_inner()?;

        Ok(owned)
    }

    pub fn as_mat(&self) -> BoxedRef<'_, Mat> {
        self.as_mat_inner().unwrap()
    }

    fn as_mat_inner(&self) -> Result<BoxedRef<'_, Mat>> {
        #[cfg(debug_assertions)]
        if let Some(inner) = self.inner.as_ref() {
            use opencv::core::{MatTraitConst, Rect};

            return Ok(inner.roi(Rect::new(0, 0, self.cols, self.rows))?);
        }

        Ok(Mat::new_rows_cols_with_bytes::<Vec4b>(
            self.rows,
            self.cols,
            &self.bytes,
        )?)
    }
}

#[cfg(debug_assertions)]
impl From<Mat> for OwnedMat {
    fn from(value: Mat) -> Self {
        use opencv::core::MatTraitConst;

        Self {
            rows: value.rows(),
            cols: value.cols(),
            bytes: vec![],
            inner: Some(value),
        }
    }
}
