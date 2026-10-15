/// A scalar SQL value, encoded for ChuSQL's literal syntax.
#[derive(Debug, Clone, Copy)]
pub enum Param<'a> {
    Text(&'a str),
    Int(i64),
    Bool(bool),
    Null,
}

impl<'a> From<&'a str> for Param<'a> {
    fn from(value: &'a str) -> Self {
        Self::Text(value)
    }
}

impl From<i64> for Param<'_> {
    fn from(value: i64) -> Self {
        Self::Int(value)
    }
}

impl From<bool> for Param<'_> {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

/// Builds SQL from trusted syntax and separately encoded values.
/// This is client-side literal encoding, not a server prepared statement.
/// Never pass user input to `new` or `push`; use `bind` for values.
#[derive(Debug, Clone)]
pub struct Query(String);

impl Query {
    /// Start with trusted SQL syntax.
    pub fn new(sql: &str) -> Self {
        Self(sql.to_owned())
    }

    /// Append trusted SQL syntax, including whitespace as needed.
    pub fn push(mut self, sql: &str) -> Self {
        self.0.push_str(sql);
        self
    }

    /// Append a scalar value. Strings escape quotes using ChuSQL's `''` rule.
    /// Does not encode table/column names, SQL expressions, or LIKE patterns.
    pub fn bind<'a>(mut self, value: impl Into<Param<'a>>) -> Self {
        match value.into() {
            Param::Text(text) => {
                self.0.push('\'');
                for ch in text.chars() {
                    self.0.push(ch);
                    if ch == '\'' {
                        self.0.push('\'');
                    }
                }
                self.0.push('\'');
            }
            Param::Int(number) => self.0.push_str(&number.to_string()),
            Param::Bool(value) => self.0.push_str(if value { "TRUE" } else { "FALSE" }),
            Param::Null => self.0.push_str("NULL"),
        }
        self
    }

    /// The encoded SQL; execute it using `client.query(query.as_str()).await`.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_text_stays_inside_one_literal() {
        let query = Query::new("INSERT INTO posts VALUES (")
            .bind("楚的博客'); DROP TABLE posts; --\n\\正文\0")
            .push(", ")
            .bind(i64::MIN)
            .push(", ")
            .bind(true)
            .push(", ")
            .bind(Param::Null)
            .push(")");
        assert_eq!(
            query.as_str(),
            "INSERT INTO posts VALUES ('楚的博客''); DROP TABLE posts; --\n\\正文\0', -9223372036854775808, TRUE, NULL)"
        );
    }

    #[test]
    fn empty_and_consecutive_quotes_are_preserved() {
        assert_eq!(Query::new("").bind("").as_str(), "''");
        assert_eq!(Query::new("").bind("''").as_str(), "''''''");
    }
}
