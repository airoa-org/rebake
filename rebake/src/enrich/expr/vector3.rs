use polars::lazy::dsl::as_struct;
use polars::prelude::*;

#[derive(Clone)]
pub(crate) struct Vector3Expr {
    x: Expr,
    y: Expr,
    z: Expr,
}

impl Vector3Expr {
    pub fn new(x: Expr, y: Expr, z: Expr) -> Self {
        Self { x, y, z }
    }

    pub fn x(&self) -> Expr {
        self.x.clone()
    }

    pub fn y(&self) -> Expr {
        self.y.clone()
    }

    pub fn z(&self) -> Expr {
        self.z.clone()
    }

    pub fn shifted(&self, offset: Expr) -> Self {
        Self::new(
            self.x.clone().shift(offset.clone()),
            self.y.clone().shift(offset.clone()),
            self.z.clone().shift(offset),
        )
    }

    pub fn delta(&self, previous: &Self) -> Self {
        Self::new(
            self.x.clone() - previous.x.clone(),
            self.y.clone() - previous.y.clone(),
            self.z.clone() - previous.z.clone(),
        )
    }

    pub fn fill_null(self, value: f64) -> Self {
        let filler = lit(value);
        Self::new(
            self.x.fill_null(filler.clone()),
            self.y.fill_null(filler.clone()),
            self.z.fill_null(filler),
        )
    }

    pub fn into_struct(self, name: &str) -> Expr {
        as_struct(vec![
            self.x.alias("x"),
            self.y.alias("y"),
            self.z.alias("z"),
        ])
        .alias(name)
    }
}
