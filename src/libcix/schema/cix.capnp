@0xeba049cb7c053dd8;

struct Order {
    id          @0 :Uuid;
    user        @1 :Uuid;
    symbol      @2 :Text;
    side        @3 :OrderSide;
    price       @4 :Float64;
    quantity    @5 :UInt32;
    updated     @6 :Timestamp;
}

struct UserExecution {
    id          @0 :Uuid;
    ts          @1 :Timestamp;
    order       @2 :Uuid;
    side        @3 :OrderSide;
    symbol      @4 :Text;
    price       @5 :Float64;
    quantity    @6 :UInt32;
}

struct Execution {
    id          @0 :Uuid;
    ts          @1 :Timestamp;
    buyer       @2 :Uuid;
    seller      @3 :Uuid;
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
    id          @0 :Uuid;
    price       @1 :Float64;
    quantity    @2 :UInt32;
}

interface TradingSession {
    authenticate @0 (user :Uuid) -> (response :AuthCode);
    newOrder @1 (order :NewOrder) -> (code :ErrorCode, id :Uuid);
    executionSubscribe @2 (feed :ExecutionFeed)
        -> (code :ErrorCode, sub :ExecutionFeedSubscription);
    #changeOrder @2 (change :ChangeOrder) -> (response :Bool);
}

interface ExecutionFeedSubscription {}

interface ExecutionFeed {
    execution @0 (execution: UserExecution) -> ();
}
