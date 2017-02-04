pub mod trade_types {
    use capnp;
    use cix_capnp as cp;
    use std::cmp::{Eq, PartialEq};
    use std::convert::From;
    use std::error;
    use std::fmt;
    use std::hash::{Hash,Hasher};
    use std::str::from_utf8;
    use time;
    use uuid;

    pub const SYMBOL_MAX_LENGTH: usize = 8;

    pub type UserId = uuid::Uuid;
    pub type OrderId = uuid::Uuid;
    pub type ExecutionId = uuid::Uuid;
    pub type Price = f64;
    pub type Quantity = u32;
    pub type OrderTime = time::Timespec;

    #[derive(Clone, Copy, Debug)]
    pub struct Symbol {
        pub s: [u8; SYMBOL_MAX_LENGTH]
    }

    impl Symbol {
        pub fn new() -> Self {
            Symbol {
                s: [0u8; SYMBOL_MAX_LENGTH]
            }
        }

        pub fn from_bytes(bytes: &[u8]) -> Result<Self, ()> {
            let mut s = Self::new();
            let sym_len = bytes.len();

            if sym_len > SYMBOL_MAX_LENGTH {
                return Err(());
            }

            s.s[..sym_len].clone_from_slice(bytes);

            Ok(s)
        }

        pub fn from_str(s: &str) -> Result<Self, ()> {
            Self::from_bytes(s.as_bytes())
        }
    }

    impl fmt::Display for Symbol {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "{}", try!(from_utf8(&self.s).map_err(|_| {
                fmt::Error
            })))
        }
    }

    #[derive(Debug)]
    pub struct Error {
        code: ErrorCode,
        desc: String
    }

    #[derive(Debug)]
    pub enum ErrorCode {
        Other
    }

    impl fmt::Display for Error {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "{:?}: {}", self.code, self.desc)
        }
    }

    impl error::Error for Error {
        fn description(&self) -> &str {
            self.desc.as_str()
        }

        fn cause(&self) -> Option<&error::Error> {
            // XXX
            None
        }
    }

    impl From<capnp::Error> for Error {
        fn from(e: capnp::Error) -> Self {
            Error::new(ErrorCode::Other, e.description)
        }
    }

    impl From<capnp::NotInSchema> for Error {
        fn from(e: capnp::NotInSchema) -> Self {
            let capnp::NotInSchema(x) = e;
            Error::new(ErrorCode::Other,
                       format!("unknown enum value {}", x))
        }
    }

    impl Error {
        fn new(code: ErrorCode, desc: String) -> Self {
            Error {
                code: code,
                desc: desc
            }
        }
    }

    #[derive(Clone, Copy, Debug)]
    pub enum OrderSide {
        Buy,
        Sell
    }

    impl Default for OrderSide {
        fn default() -> Self { OrderSide::Buy }
    }

    impl From<cp::OrderSide> for OrderSide {
        fn from(o: cp::OrderSide) -> Self {
            match o {
                cp::OrderSide::Buy => OrderSide::Buy,
                cp::OrderSide::Sell => OrderSide::Sell
            }
        }
    }

    #[derive(Clone, Copy, Debug)]
    pub struct Order {
        pub id:         OrderId,
        pub user:       UserId,
        pub symbol:     Symbol,
        pub side:       OrderSide,
        pub price:      Price,
        pub quantity:   Quantity,
        pub update:     OrderTime
    }

    impl Default for Order {
        fn default() -> Self {
            Order {
                id:         uuid::Uuid::default(),
                user:       uuid::Uuid::default(),
                symbol:     Symbol::new(),
                side:       OrderSide::Buy,
                price:      0f64,
                quantity:   0u32,
                update:     time::now().to_timespec()
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

    fn read_uuid(r: cp::uuid::Reader) -> Result<uuid::Uuid, Error> {
        let bytes = try!(r.get_bytes().map_err(|e| {
            Error::new(ErrorCode::Other, "missing bytes".to_string())
        }));

        let res = try!(uuid::Uuid::from_bytes(bytes).map_err(|e| {
            match e {
                uuid::ParseError::InvalidLength(n) => {
                    Error::new(ErrorCode::Other,
                               format!("invalid byte length {}", n))
                },
                _ => {
                    Error::new(ErrorCode::Other, "unknown error".to_string())
                }
            }
        }));

        Ok(res)
    }

    fn read_timestamp(r: cp::timestamp::Reader) -> time::Timespec {
        time::Timespec {
            sec:    r.get_seconds(),
            nsec:   r.get_nanos()
        }
    }

    fn read_symbol(r: capnp::text::Reader) -> Result<Symbol, Error> {
        let raw_sym = r.as_bytes();

        Symbol::from_bytes(raw_sym).map_err(|e| {
            Error::new(ErrorCode::Other, "invalid symbol".to_string())
        })
    }

    fn read_order(o: cp::order::Reader) -> Result<Order, Error> {
        // XXX: learn how rust macros work
        let id_bytes = try!(o.get_id());
        let id_uuid = try!(read_uuid(id_bytes));
        let user_bytes = try!(o.get_user());
        let user_uuid = try!(read_uuid(user_bytes));
        let sym_str = try!(o.get_symbol());

        Ok(Order {
            id:         id_uuid,
            user:       user_uuid,
            symbol:     try!(read_symbol(sym_str)),
            side:       OrderSide::from(try!(o.get_side())),
            price:      o.get_price(),
            quantity:   o.get_quantity(),
            update:     read_timestamp(try!(o.get_updated()))
        })
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
