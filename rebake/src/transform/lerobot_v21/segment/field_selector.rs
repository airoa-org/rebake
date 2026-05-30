use polars::prelude::*;
use std::str::FromStr;

#[derive(Debug, PartialEq, Clone)]
pub enum FieldSelector {
    Key(String),
    Index(i64),
    Slice {
        start: Option<i64>,
        end: Option<i64>,
    },
}

impl FromStr for FieldSelector {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Ok(index) = s.parse::<i64>() {
            return Ok(FieldSelector::Index(index));
        }

        if s.contains(':') {
            let parts: Vec<&str> = s.split(':').collect();
            if parts.len() > 2 {
                return Err(format!("Invalid slice format: {}", s));
            }

            let start = if parts[0].is_empty() {
                None
            } else {
                Some(
                    parts[0]
                        .parse::<i64>()
                        .map_err(|_| format!("Invalid start index: {}", parts[0]))?,
                )
            };

            let end = if parts.len() > 1 && !parts[1].is_empty() {
                Some(
                    parts[1]
                        .parse::<i64>()
                        .map_err(|_| format!("Invalid end index: {}", parts[1]))?,
                )
            } else {
                None
            };

            return Ok(FieldSelector::Slice { start, end });
        }

        Ok(FieldSelector::Key(s.to_string()))
    }
}

pub fn build_field_expression(field_path: &str) -> Result<Expr, String> {
    if !field_path.starts_with('/') {
        return Err("field paths must start with '/'".into());
    }

    let segments: Vec<&str> = field_path.split('/').skip(1).collect();
    if segments.is_empty() || (segments.len() == 1 && segments[0].is_empty()) {
        return Err("field path cannot be empty".into());
    }

    let selectors: Result<Vec<FieldSelector>, String> =
        segments.into_iter().map(FieldSelector::from_str).collect();
    let selectors = selectors?;
    build_recursive(selectors)
}

fn build_recursive(selectors: Vec<FieldSelector>) -> Result<Expr, String> {
    if selectors.is_empty() {
        return Err("Empty selectors".to_string());
    }

    // INVARIANT: selectors is not empty (checked above), so split_first always succeeds
    #[allow(clippy::unwrap_used)]
    let (first, tail) = selectors.split_first().unwrap();
    let mut current_expr = match first {
        FieldSelector::Key(name) => col(name),
        _ => return Err("First selector must be a Key (column name)".to_string()),
    };

    let mut is_mapped_list = false; // Assume root is NOT a mapped list context

    for selector in tail {
        current_expr = match selector {
            FieldSelector::Key(name) => handle_key(current_expr, name, is_mapped_list),
            FieldSelector::Index(index) => handle_index(current_expr, *index, &mut is_mapped_list),
            FieldSelector::Slice { start, end } => {
                handle_slice(current_expr, *start, *end, &mut is_mapped_list)
            }
        };
    }

    Ok(current_expr)
}

fn handle_key(expr: Expr, name: &str, is_mapped_list: bool) -> Expr {
    if is_mapped_list {
        expr.list().eval(col("").struct_().field_by_name(name))
    } else {
        expr.struct_().field_by_name(name)
    }
}

fn handle_index(expr: Expr, index: i64, is_mapped_list: &mut bool) -> Expr {
    if *is_mapped_list {
        // Vectorized index: Get i-th element of each list in the current list
        expr.list().eval(col("").list().get(lit(index), true))
    } else {
        // Direct index: Get i-th element of the list
        *is_mapped_list = false;
        expr.list().get(lit(index), true)
    }
}

fn handle_slice(
    expr: Expr,
    start: Option<i64>,
    end: Option<i64>,
    is_mapped_list: &mut bool,
) -> Expr {
    let offset = start.unwrap_or(0);
    let len_expr = match (start, end) {
        (Some(s), Some(e)) => lit(e - s),
        (Some(s), None) => expr.clone().list().len() - lit(s),
        (None, Some(e)) => lit(e),
        (None, None) => expr.clone().list().len(),
    };

    *is_mapped_list = true;
    expr.list().slice(lit(offset), len_expr)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_build_field_expression_execution() {
        let item1 = Series::new("item".into(), &[0, 1, 2, 3, 4]);
        let item2 = Series::new("item".into(), &[10, 11, 12]);
        let list_ca: ListChunked = vec![Some(item1), Some(item2)].into_iter().collect();
        let s_list = list_ca.into_series().with_name("data".into());

        let df = DataFrame::new(vec![s_list.into()]).unwrap();

        let expr = build_field_expression("/data/1:3").unwrap();
        let result = df
            .clone()
            .lazy()
            .select([expr.alias("res")])
            .collect()
            .unwrap();
        let res_col = result.column("res").unwrap();

        let row0 = res_col.get(0).unwrap();
        if let AnyValue::List(s) = row0 {
            assert_eq!(s.len(), 2);
            assert_eq!(s.get(0).unwrap(), AnyValue::Int32(1));
            assert_eq!(s.get(1).unwrap(), AnyValue::Int32(2));
        } else {
            panic!("Expected List");
        }

        let inner1 = Series::new("a".into(), &[0, 1]);
        let inner2 = Series::new("a".into(), &[2, 3]);
        let inner3 = Series::new("a".into(), &[4, 5]);

        let l1_inner = vec![
            Some(inner1.clone()),
            Some(inner2.clone()),
            Some(inner3.clone()),
        ];
        let l1_ca: ListChunked = l1_inner.into_iter().collect();
        let l1 = l1_ca.into_series();

        let l2_inner = vec![Some(inner1.clone())];
        let l2_ca: ListChunked = l2_inner.into_iter().collect();
        let l2 = l2_ca.into_series();

        let nested_ca: ListChunked = vec![Some(l1), Some(l2)].into_iter().collect();
        let s_nested = nested_ca.into_series().with_name("nested".into());

        let df_nested = DataFrame::new(vec![s_nested.into()]).unwrap();

        let expr = build_field_expression("/nested/0:2/0").unwrap();
        let result = df_nested
            .lazy()
            .select([expr.alias("res")])
            .collect()
            .unwrap();
        let res_col = result.column("res").unwrap();

        let row0 = res_col.get(0).unwrap();
        if let AnyValue::List(s) = row0 {
            assert_eq!(s.len(), 2);
            assert_eq!(s.get(0).unwrap(), AnyValue::Int32(0));
            assert_eq!(s.get(1).unwrap(), AnyValue::Int32(2));
        } else {
            panic!("Expected List, got {:?}", row0);
        }
    }
}
