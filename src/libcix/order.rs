use std::core::default::Default;
use std::fmt;
use uuid::Uuid;

mod cix {
    type UserId = Uuid;
    type OrderId = Uuid;
    type ExecutionId = Uuid;
    type Price = f64;
    type Quantity = u32;
    type Symbol = &str;

    pub enum OrderSide {
        Buy,
        Sell
    }

    #[derive(Default)]
    pub struct Order {
        user:       UserId,
        id:         OrderId,
        symbol:     Symbol,
        side:       OrderSide,
        price:      Price,
        quantity:   Quantity
    }

    impl Eq for Order {
        fn eq(&self, other: &Order) -> bool {
            self.id == other.id
        }
    }

    impl Hash for Order {
        fn hash<H: Hasher>(&self, state: &mut h) {
            self.id.hash(state)
        }
    }

    impl Ord for Order {
        fn cmp(&self, other: Order) -> Ordering {
            assert_eq!(self.side, other.side);
            let price_diff = match self.side {
                Buy  => self.price - other.price,
                Sell => other.price - self.price
            }

            match price_diff.cmp(0) {
                Greater => Greater,
                Less    => Less,
                Equal   => self.time.cmp(other.time)
            }
        }
    }

    pub struct Execution {
        id:         ExecutionId,
        buy_order:  OrderId,
        sell_order: OrderId,
        symbol:     Symbol,
        price:      Price,
        quantity:   Quantity
    }

    impl fmt::Display for Execution {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "Execution {}: {} bought {} shares of {} from {} @ {}",
                   self.id, self.buy_order.id, self.sell_order.id,
                   self.quantity, self.symbol, self.price)
        }
    }
}
