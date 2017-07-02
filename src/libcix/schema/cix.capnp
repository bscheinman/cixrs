@0xeba049cb7c053dd8;

struct Order {
    id          @0 :UInt64;
    user        @1 :UInt64;
    symbol      @2 :Text;
    side        @3 :OrderSide;
    price       @4 :Float64;
    quantity    @5 :UInt32;
    updated     @6 :Timestamp;
}

struct UserExecution {
    id          @0 :UInt64;
    ts          @1 :Timestamp;
    order       @2 :UInt64;
    side        @3 :OrderSide;
    symbol      @4 :Text;
    price       @5 :Float64;
    quantity    @6 :UInt32;
}

struct Execution {
    id          @0 :UInt64;
    ts          @1 :Timestamp;
    buyer       @2 :UInt64;
    seller      @3 :UInt64;
    symbol      @4 :Text;
    price       @5 :Float64;
    quantity    @6 :UInt32;
}

enum OrderSide {
    buy @0;
    sell @1;
}

# Is there a way to specify fixed-length arrays?
struct Uuid {
    bytes       @0 :Data;
}

struct Timestamp {
    seconds     @0 :Int64;
    nanos       @1 :Int32;
}

enum ErrorCode {
    ok @0;
    notAuthenticated @1;
    alreadySubscribed @2;
    invalidArgs @3;
    other @4;
}

enum AuthCode {
    ok @0;
    invalid @1;
}

struct NewOrder {
    symbol      @0 :Text;
    side        @1 :OrderSide;
    price       @2 :Float64;
    quantity    @3 :UInt32;
}

struct ChangeOrder {
    id          @0 :UInt64;
    price       @1 :Float64;
    quantity    @2 :UInt32;
}

struct CancelOrder {
    id          @0 :UInt64;
}

interface TradingSession {
    authenticate @0 (user :UInt64) -> (response :AuthCode);
    newOrder @1 (order :NewOrder) -> (code :ErrorCode, id :UInt64);
    executionSubscribe @2 (feed :ExecutionFeed)
        -> (code :ErrorCode, sub :ExecutionFeedSubscription);
    cancelOrder @3 (cancel :CancelOrder) -> (code :ErrorCode);
    getOpenOrders @4 () -> (code :ErrorCode, orders :List(Order));
    #changeOrder @4 (change :ChangeOrder) -> (code :ErrorCode);
}

interface ExecutionFeedSubscription {}

interface ExecutionFeed {
    execution @0 (execution: UserExecution) -> ();
}
