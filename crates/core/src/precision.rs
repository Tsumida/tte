use getset::Getters;
use rust_decimal::Decimal;

#[derive(Clone, Debug, Getters)]
pub struct PrecisionContext {
    #[get = "pub"]
    precision: u32,
}

impl PrecisionContext {
    pub fn new(precision: u32) -> Self {
        Self { precision }
    }

    pub fn truncate(&self, v: Decimal) -> Decimal {
        v.round_dp(self.precision)
    }
}

#[cfg(test)]
mod test {
    use rust_decimal::Decimal;

    #[test]
    fn test_with_precision() {
        let value = Decimal::new(123456, 4); // 12.3456
        assert_eq!(
            super::PrecisionContext::new(2).truncate(value),
            Decimal::new(1235, 2)
        ); // 12.35
        assert_eq!(
            super::PrecisionContext::new(3).truncate(value),
            Decimal::new(12346, 3)
        ); // 12.346
    }
}
