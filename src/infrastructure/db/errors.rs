//! Classification of sqlx errors into application errors.

use crate::application::AppError;

/// SQLSTATE classes that mean "the database is not reachable right now":
/// connection exceptions (`08xxx`) and server shutdown states
/// (`57P01` admin shutdown, `57P02` crash shutdown, `57P03` cannot connect now).
fn is_unavailable_sqlstate(code: &str) -> bool {
    code.starts_with("08") || matches!(code, "57P01" | "57P02" | "57P03")
}

/// Pool timeouts, closed pools, transport failures and connection-class
/// SQLSTATEs become [`AppError::Unavailable`] (HTTP 503); everything else is
/// [`AppError::Internal`] (HTTP 500).
pub fn map_sqlx_error(err: sqlx::Error) -> AppError {
    let unavailable = match &err {
        sqlx::Error::PoolTimedOut | sqlx::Error::PoolClosed | sqlx::Error::Io(_) => true,
        sqlx::Error::Tls(_) => true,
        sqlx::Error::Database(db_err) => db_err.code().is_some_and(|c| is_unavailable_sqlstate(&c)),
        _ => false,
    };
    if unavailable {
        AppError::Unavailable(Box::new(err))
    } else {
        AppError::Internal(Box::new(err))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_timeout_is_unavailable() {
        assert!(matches!(
            map_sqlx_error(sqlx::Error::PoolTimedOut),
            AppError::Unavailable(_)
        ));
    }

    #[test]
    fn pool_closed_is_unavailable() {
        assert!(matches!(
            map_sqlx_error(sqlx::Error::PoolClosed),
            AppError::Unavailable(_)
        ));
    }

    #[test]
    fn io_error_is_unavailable() {
        let io = std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "refused");
        assert!(matches!(
            map_sqlx_error(sqlx::Error::Io(io)),
            AppError::Unavailable(_)
        ));
    }

    #[test]
    fn row_not_found_is_internal() {
        assert!(matches!(
            map_sqlx_error(sqlx::Error::RowNotFound),
            AppError::Internal(_)
        ));
    }

    #[test]
    fn connection_class_sqlstates_are_unavailable() {
        for code in ["08000", "08006", "57P01", "57P02", "57P03"] {
            assert!(is_unavailable_sqlstate(code), "{code}");
        }
        for code in ["22012", "23514", "42P01", "57014"] {
            assert!(!is_unavailable_sqlstate(code), "{code}");
        }
    }
}
