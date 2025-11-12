//!
//! ID Generator
//!
//!

pub struct IDGenerator;

impl IDGenerator {
    pub fn gen_order_id(account_id: u64) -> String {
        format!(
            "O{:07}N{}",
            Self::account_id_hash(account_id) % 10000000,
            // uuid without -
            uuid::Uuid::new_v4()
                .to_string()
                .to_uppercase()
                .replace("-", ""),
        )
    }

    #[inline]
    fn account_id_hash(account_id: u64) -> u64 {
        // id ^ secret
        account_id ^ 0x5A5A5A5A5A5A5A5A
    }
}

#[cfg(test)]
mod tests {
    use super::IDGenerator;

    #[test]
    fn test_gen_order_id() {
        let account_id = 123456789;
        let order_id = IDGenerator::gen_order_id(account_id);
        println!("Generated Order ID: {}", order_id);
        assert!(order_id.starts_with("O"));
    }
}
