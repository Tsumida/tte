# Prompt: Design a Standalone Reliable Log Replicator (RLR)
I am designing a standalone distributed sequencer / reliable log replicator, named RLR, implemented in Rust, based on OpenRaft and RocksDB.
 RLR is intended to provide high availability and strict ordering guarantees for upstream business systems such as OMS (Order Management System) and ME (Matching Engine).
RLR is not an embedded Raft component inside OMS or ME.
Instead, it is a separate service that acts as the single ordering authority.
OMS and ME act as state machine clients, consuming committed logs from RLR.
Please reason strictly within the following design constraints and goals.

---
# 1. Core Concept
- RLR is a reliable log replicator / sequencer
- OMS / ME:
  - Do not participate in Raft
  - Do not store Raft logs
  - Only consume committed log entries
- RLR:
  - Owns leader election
  - Owns log replication
  - Owns commit ordering
- All business state lives in OMS / ME, not in RLR

---
# 2. State Machine Model
- The “state machine” refers to business logic systems (OMS / ME)
- Each state machine must be role-aware:
  - Leader
  - Follower
  - Learner
- State machines apply committed logs deterministically
- State machines may conditionally emit side effects based on role

---
# 3. Storage Layer
RLR uses RocksDB and separates concerns clearly:
1. LogStorage
  - Stores Raft log entries
  - Append-only
  - Written only by the Leader
2. MetaStorage
  - Stores:
    - Cluster configuration
    - Current term
    - Vote
    - Last applied index
3. Snapshot
  - Used only for follower / learner bootstrap
  - Does not contain OMS / ME business state

---
# 4. Network Layer
- gRPC over HTTP/2
- Supports:
  - Vote
  - AppendEntries (batch log replication)
  - Snapshot installation
  - ChangeMembership
  - AddLearner
- Supports bidirectional streaming where appropriate

---
# 5. Ingress Rules (Write Path)
- Only the Leader is allowed to accept propose requests
- Proposals sent to non-leader nodes must be:
  - Rejected, or
  - Rejected with leader redirection information
- Followers and learners must not mutate ordering

---
# 6. Egress Rules (Read / Output Path)
- All nodes apply committed logs
- Only specific roles are allowed to emit side effects externally
Example behavior matrix:

| Role     | Input (Propose) | Ingress | Apply Logs | Output Events    | Egress |
|----------|------------------|---------|------------|------------------|--------|
| Leader   | Order / Cancel   | Accept  | Apply      | Emit             | Keep   |
| Follower | Order / Cancel   | Drop    | Apply      | Suppress         | Drop   |
| Learner  | Order / Cancel   | Drop    | Apply      | Suppress         | Drop   |
| Learner  | —                | —       | Apply      | Persist to DB    | Keep   |

Outputs may include:
- order_event
- ledger_event
- match_result
- or persistence to MySQL for verification

---
# 7. Operational Scenarios
1. Data Synchronization
  - Learners are used for full data sync and verification
2. Node Replacement / Upgrade
  - New node joins as learner
  - Outputs are verified (e.g. hash comparison)
  - After verification, old node is removed

---
# 8. External API Requirements
RLR should expose a minimal, clean API, including:
- propose(command) — leader-only
- subscribe(from_index) — stream committed logs
- get_role() — Leader / Follower / Learner
- get_leader() — leader discovery
- get_cluster_info()
State machines must be able to know the role context when applying logs.

---
# 9. Leader Routing Requirement
Please reason about how client requests (e.g. OMS order submissions) are routed to the current leader:
- Should clients be leader-aware?
- Should followers forward requests?
- Is an external routing layer required?
- Why L4 load balancers are insufficient
- Trade-offs between:
  - Client-side leader discovery
  - Follower forwarding
  - Dedicated routing proxy

---
# 10. Goal of the Design
The final design should:
- Be production-grade
- Be easy to operate and reason about
- Avoid unnecessary indirection
- Favor correctness and observability over convenience
- Support future extensions such as:
  - Exactly-once apply
  - Deterministic replay
  - Auditing and reconciliation

---
# 11. Code style

Role: You are an expert Rust Engineer specializing in systems programming, API design, and high-performance safety. Your goal is to provide Rust code that is idiomatic, secure, and easy to maintain.

Instructions: When writing Rust code or designing APIs, strictly adhere to the following principles:

1. Code Clarity & Complexity Control
Favor Composition: Use traits and composition over complex nested generics where possible.

Idiomatic Patterns: Use Option, Result, and the ? operator for error handling. Avoid unwrap() or expect() unless in test code or documenting an "impossible" state.

Functional Style: Use iterators (map, filter, collect) instead of manual for loops when it improves readability.

Naming: Follow standard naming conventions (CamelCase for types, snake_case for functions/variables).

2. Security & Memory Safety
No unsafe: Do not use unsafe blocks unless there is a documented, unavoidable performance requirement.

Ownership & Borrowing: Prefer borrowing (&T) over cloning (.clone()) unless ownership is strictly required. Use Cow<'a, T> for optimizing allocations when appropriate.

Data Validation: Implement the "Parse, don't validate" pattern. Use types to enforce invariants so that invalid states are unrepresentable.

3. API Design
Builder Pattern: Use the Builder pattern for structs with many optional configurations.

Visibility: Keep fields private by default. Expose functionality through well-documented public methods.

Crate Selection: Prefer standard library solutions. If external crates are needed, prioritize "de facto" industry standards (e.g., serde, tokio, anyhow, thiserror).

4. Debugging & Observability
Error Types: Use thiserror for library errors (structured enums) and anyhow for application-level error handling.

Tracing: Integrate tracing or log macros to provide visibility into runtime execution.

Derive Debug: Always #[derive(Debug)] for public structs and enums to ensure they are easy to inspect.

5. Documentation & Testing
Doc Comments: Provide /// documentation for all public items, including "Examples" and "Errors" sections.

Unit Tests: Include a cfg(test) module with meaningful test cases for the core logic.

How to use this prompt effectively
When you use the prompt above, you can append your specific task at the end. For example:

"Using the guidelines above, please design a thread-safe, asynchronous LRU cache API that stores web session data."

Why this works:
Safety First: By explicitly forbidding unsafe and unwrap, you force the LLM to handle edge cases and use the borrow checker correctly.

Maintainability: Mentioning "Parse, don't validate" encourages the LLM to use Rust's type system (like Newtypes or Enums) to prevent bugs before they happen.

Debug-Ready: Forcing #[derive(Debug)] and the use of the tracing crate ensures that the generated code isn't a "black box."

---