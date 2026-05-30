use polars::lazy::dsl::{as_struct, when};
use polars::prelude::*;

use super::vector3::Vector3Expr;

#[derive(Clone)]
pub(crate) struct QuaternionExpr {
    w: Expr,
    x: Expr,
    y: Expr,
    z: Expr,
}

impl QuaternionExpr {
    pub fn new(w: Expr, x: Expr, y: Expr, z: Expr) -> Self {
        Self { w, x, y, z }
    }

    pub fn shifted(&self, offset: Expr) -> Self {
        Self::new(
            self.w.clone().shift(offset.clone()),
            self.x.clone().shift(offset.clone()),
            self.y.clone().shift(offset.clone()),
            self.z.clone().shift(offset),
        )
    }

    pub fn dot(&self, other: &Self) -> Expr {
        self.w.clone() * other.w.clone()
            + self.x.clone() * other.x.clone()
            + self.y.clone() * other.y.clone()
            + self.z.clone() * other.z.clone()
    }

    pub fn scaled(&self, factor: Expr) -> Self {
        Self::new(
            self.w.clone() * factor.clone(),
            self.x.clone() * factor.clone(),
            self.y.clone() * factor.clone(),
            self.z.clone() * factor,
        )
    }

    pub fn align_to_shortest_arc(&self, reference: &Self) -> Self {
        let factor = when(self.dot(reference).lt(lit(0.0)))
            .then(lit(-1.0))
            .otherwise(lit(1.0));
        self.scaled(factor)
    }

    /// Rotates a source-frame vector into this quaternion's local frame.
    ///
    /// This assumes a unit quaternion. TF-derived rotations should satisfy that
    /// contract, so this intentionally avoids per-row normalization.
    pub fn inverse_rotate_vector(&self, vector: &Vector3Expr) -> Vector3Expr {
        let one = lit(1.0);
        let two = lit(2.0);

        let xx = self.x.clone() * self.x.clone();
        let yy = self.y.clone() * self.y.clone();
        let zz = self.z.clone() * self.z.clone();
        let xy = self.x.clone() * self.y.clone();
        let xz = self.x.clone() * self.z.clone();
        let yz = self.y.clone() * self.z.clone();
        let xw = self.x.clone() * self.w.clone();
        let yw = self.y.clone() * self.w.clone();
        let zw = self.z.clone() * self.w.clone();

        let vx = vector.x();
        let vy = vector.y();
        let vz = vector.z();

        let x = (one.clone() - two.clone() * yy.clone() - two.clone() * zz.clone()) * vx.clone()
            + (two.clone() * xy.clone() + two.clone() * zw.clone()) * vy.clone()
            + (two.clone() * xz.clone() - two.clone() * yw.clone()) * vz.clone();
        let y = (two.clone() * xy.clone() - two.clone() * zw.clone()) * vx.clone()
            + (one.clone() - two.clone() * xx.clone() - two.clone() * zz) * vy.clone()
            + (two.clone() * yz.clone() + two.clone() * xw.clone()) * vz.clone();
        let z = (two.clone() * xz + two.clone() * yw) * vx
            + (two.clone() * yz - two.clone() * xw) * vy
            + (one - two.clone() * xx - two * yy) * vz;

        Vector3Expr::new(x, y, z)
    }

    pub fn delta(&self, previous: &Self) -> Self {
        let w = previous.w.clone() * self.w.clone()
            + previous.x.clone() * self.x.clone()
            + previous.y.clone() * self.y.clone()
            + previous.z.clone() * self.z.clone();
        let x = previous.w.clone() * self.x.clone()
            - previous.x.clone() * self.w.clone()
            - previous.y.clone() * self.z.clone()
            + previous.z.clone() * self.y.clone();
        let y = previous.w.clone() * self.y.clone() + previous.x.clone() * self.z.clone()
            - previous.y.clone() * self.w.clone()
            - previous.z.clone() * self.x.clone();
        let z = previous.w.clone() * self.z.clone() - previous.x.clone() * self.y.clone()
            + previous.y.clone() * self.x.clone()
            - previous.z.clone() * self.w.clone();
        Self::new(w, x, y, z)
    }

    pub fn fill_null(self, vector_value: f64, scalar_value: f64) -> Self {
        Self::new(
            self.w.fill_null(lit(scalar_value)),
            self.x.fill_null(lit(vector_value)),
            self.y.fill_null(lit(vector_value)),
            self.z.fill_null(lit(vector_value)),
        )
    }

    pub fn into_struct(self, name: &str) -> Expr {
        as_struct(vec![
            self.x.alias("x"),
            self.y.alias("y"),
            self.z.alias("z"),
            self.w.alias("w"),
        ])
        .alias(name)
    }
}
