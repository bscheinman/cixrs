pub mod trade_types {
    use std::cmp::{Eq, PartialEq};
    use std::fmt;
    use std::hash::{Hash,Hasher};
    use std::time::SystemTime;
    use uuid::Uuid;

    pub type UserId = Uuid;
    pub type OrderId = Uuid;
    pub type ExecutionId = Uuid;
    pub type Price = f64;
    pub type Quantity = u32;
    pub type Symbol = &'static str;
    pub type OrderTime = SystemTime;

    #[derive(Clone, Copy, Debug)]
    pub enum OrderSide {
        Buy,
        Sell
    }

    impl Default for OrderSide {
        fn default() -> Self { OrderSide::Buy } 
    }

    #[derive(Clone, Copy, Debug)]
    pub struct Order {
        pub user:       UserId,
        pub id:         OrderId,
        pub symbol:     Symbol,
        pub side:       OrderSide,
        pub price:      Price,
        pub quantity:   Quantity,
        pub update:     OrderTime
    }

    impl Default for Order {
        fn default() -> Self {
            Order {
                user:       Uuid::default(),
                id:         Uuid::default(),
                symbol:     "",
                side:       OrderSide::Buy,
                price:      0f64,
                quantity:   0u32,
                update:     SystemTime::now()
            }
        }
    }

    impl PartialEq<Order> for Order {
        fn eq(&self, other: &Order) -> bool {
            self.id == other.id
        }
    }
    impl Eq for Order {}

    impl Hash for Order {
        fn hash<H: Hasher>(&self, state: &mut H) {
            self.id.hash(state)
        }
    }

    pub struct Execution {
        pub id:         ExecutionId,
        pub buy_order:  OrderId,
        pub sell_order: OrderId,
        pub symbol:     Symbol,
        pub price:      Price,
        pub quantity:   Quantity
    }

    impl fmt::Display for Order {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "Order {}: {:?} {} shares of {} @ {}",
                   self.id, self.side, self.quantity, self.symbol, self.price)
        }
    }

    impl fmt::Display for Execution {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "Execution {}: {} bought {} shares of {} from {} @ {}",
                   self.id, self.buy_order, self.quantity, self.symbol,
                   self.sell_order, self.price)
        }
    }
}
