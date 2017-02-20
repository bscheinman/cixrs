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
    pub type Price = f64;
    pub type Quantity = u32;
    pub type OrderTime = time::Timespec;

    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub struct TradingId {
        val: u64
    }

    // XXX: There has to be some package that wraps this all up nicely
    const SYMBOL_BITS:              usize = 20;
    const SYMBOL_MAX:               u32 = (1u32 << SYMBOL_BITS) - 1;

    const METADATA_BITS:            usize = 4;
    const METADATA_MAX:             u8 = (1u8 << METADATA_BITS) - 1;

    const SEQUENCE_BITS:            usize = 40;
    const SEQUENCE_MAX:             u64 = (1u64 << SEQUENCE_BITS) - 1;

    //static_assert!(SYMBOL_BITS + METADATA_BITS + SEQUENCE_BITS == 64);

    const SEQUENCE_OFFSET:          usize = 0;
    const METADATA_OFFSET:          usize = SEQUENCE_OFFSET + SEQUENCE_BITS;
    const SYMBOL_OFFSET:            usize = METADATA_OFFSET + METADATA_BITS;

    const TRADING_MD_TYPE_MASK:     u8 = 1u8;
    const TRADING_MD_TYPE_ORDER:    u8 = 0u8;
    const TRADING_MD_TYPE_EXEC:     u8 = 1u8;

    const ORDER_MD_SIDE_MASK:       u8 = 2u8;
    const ORDER_MD_SIDE_BUY:        u8 = 2u8;
    const ORDER_MD_SIDE_SELL:       u8 = 0u8;

    // IDs are represented as 64-bit values with the following structure:
    // [====Symbol ID====][====metadata===][========sequence #=============]
    //       20 bits            4 bits               40 bits
    // However, clients should treat these as opaque values whose structure
    // is subject to change in the future.
    // The least significant metadata bit is 0 for orders and 1 for executions.
    // The second least significant metadata bit is 1 for buy and 0 for sell on orders and is
    // unused on executions.
    // The two remaining metadata bits are reserved for future use>
    impl TradingId {
        pub fn new(symbol_id: u32, metadata: u8, seq: u64) -> Result<Self, String> {
            if symbol_id > SYMBOL_MAX {
                return Err("symbol ID too high".to_string());
            }

            if metadata > METADATA_MAX {
                return Err("metadata value too high".to_string());
            }

            if seq > SEQUENCE_MAX {
                return Err("sequence number too high".to_string());
            }

            let val =
                ((seq as u64)       << SEQUENCE_OFFSET) |
                ((metadata as u64)  << METADATA_OFFSET) |
                ((symbol_id as u64) << SYMBOL_OFFSET);

            Ok(TradingId {
                val: val
            })
        }

        pub fn from_raw(raw: u64) -> Self {
            TradingId {
                val: raw
            }
        }

        pub fn raw(&self) -> u64 {
            self.val
        }

        pub fn symbol_id(&self) -> u32 {
            ((self.val >> SYMBOL_OFFSET) & (SYMBOL_MAX as u64)) as u32
        }

        pub fn metadata(&self) -> u8 {
            ((self.val >> METADATA_OFFSET) & (METADATA_MAX as u64)) as u8
        }

        pub fn sequence(&self) -> u64 {
            ((self.val >> SEQUENCE_OFFSET) & (SEQUENCE_MAX as u64)) as u64
        }
    }

    impl fmt::Display for TradingId {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "{}", self.val)
        }
    }

    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub struct OrderId {
        id: TradingId
    }

    impl OrderId  {
        pub fn new(symbol_id: u32, side: OrderSide, seq: u64) -> Result<Self, String> {
            let md = TRADING_MD_TYPE_ORDER | match side {
                OrderSide::Buy => ORDER_MD_SIDE_BUY,
                OrderSide::Sell => ORDER_MD_SIDE_SELL
            };

            Ok(OrderId {
                id: try!(TradingId::new(symbol_id, md, seq))
            })
        }

        pub fn from_raw(raw: u64) -> Result<Self, String> {
            let id = TradingId::from_raw(raw);
            if (id.metadata() & TRADING_MD_TYPE_MASK) != TRADING_MD_TYPE_ORDER {
                return Err("id does not represent order".to_string());
            }

            Ok(OrderId {
                id: id
            })
        }

        pub fn raw(&self) -> u64 {
            self.id.raw()
        }

        pub fn symbol_id(&self) -> u32 {
            self.id.symbol_id()
        }

        pub fn side(&self) -> OrderSide {
            match self.id.metadata() & ORDER_MD_SIDE_MASK {
                ORDER_MD_SIDE_BUY =>    OrderSide::Buy,
                _ =>                    OrderSide::Sell
            }
        }

        pub fn sequence(&self) -> u64 {
            self.id.sequence()
        }
    }

    impl Default for OrderId {
        fn default() -> Self { Self::new(SYMBOL_MAX, OrderSide::Buy, SEQUENCE_MAX).unwrap() }
    }

    impl fmt::Display for OrderId {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "{}", self.id)
        }
    }

    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub struct ExecutionId {
        id: TradingId
    }

    impl ExecutionId {
        pub fn new(symbol_id: u32, seq: u64) -> Result<Self, String> {
            let md = TRADING_MD_TYPE_EXEC;
            Ok(ExecutionId {
                id: try!(TradingId::new(symbol_id, md, seq))
            })
        }

        pub fn from_raw(raw: u64) -> Result<Self, String> {
            let id = TradingId::from_raw(raw);
            if (id.metadata() & TRADING_MD_TYPE_MASK) != TRADING_MD_TYPE_EXEC {
                return Err("id does not represent execution".to_string());
            }

            Ok(ExecutionId {
                id: id
            })
        }

        pub fn raw(&self) -> u64 {
            self.id.raw()
        }

        pub fn symbol_id(&self) -> u32 {
            self.id.symbol_id()
        }

        pub fn sequence(&self) -> u64 {
            self.id.sequence()
        }
    }

    impl Default for ExecutionId {
        fn default() -> Self { Self::new(SYMBOL_MAX, SEQUENCE_MAX).unwrap() }
    }

    impl fmt::Display for ExecutionId {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "{}", self.id)
        }
    }

    // XXX: These dervied traits rely on the assumption that all bytes after the
    // initial NUL byte will also be NUL, but we can maintain that invariant
    // below
    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
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

        pub fn as_str(&self) -> &str {
            from_utf8(&self.s).unwrap()
        }
    }

    impl fmt::Display for Symbol {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "{}", self.as_str())
        }
    }

    impl Default for Symbol {
        fn default() -> Self { Self::from_str("").unwrap() }
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
                id:         OrderId::default(),
                user:       UserId::default(),
                symbol:     Symbol::default(),
                side:       OrderSide::default(),
                price:      Price::default(),
                quantity:   Quantity::default(),
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

    pub fn read_uuid(r: cp::uuid::Reader) -> Result<uuid::Uuid, Error> {
        let bytes = try!(r.get_bytes().map_err(|_| {
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

    pub fn read_timestamp(r: cp::timestamp::Reader) -> time::Timespec {
        time::Timespec {
            sec:    r.get_seconds(),
            nsec:   r.get_nanos()
        }
    }

    pub fn read_symbol(r: capnp::text::Reader) -> Result<Symbol, Error> {
        let raw_sym = r.as_bytes();

        Symbol::from_bytes(raw_sym).map_err(|_| {
            Error::new(ErrorCode::Other, "invalid symbol".to_string())
        })
    }

    pub struct Execution {
        pub id:         ExecutionId,
        pub ts:         OrderTime,
        pub buy_order:  OrderId,
        pub buy_user:   UserId,
        pub sell_order: OrderId,
        pub sell_user:  UserId,
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
