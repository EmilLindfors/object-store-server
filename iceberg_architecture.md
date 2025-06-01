# Iceberg-Like Features Architecture

## Core Components

### 1. Catalog Service (Database-Backed)
- **Table Registry**: Track all tables and their current state
- **Schema Management**: Handle schema evolution with full lineage
- **Snapshot Management**: Time travel and versioning capabilities
- **Transaction Coordination**: ACID guarantees across multiple files

### 2. Enhanced Repository Layer
```rust
// New traits for Iceberg-like features
pub trait CatalogRepository {
    async fn create_table(&self, namespace: &str, name: &str, schema: &Schema) -> Result<TableId>;
    async fn get_table(&self, namespace: &str, name: &str) -> Result<Table>;
    async fn list_tables(&self, namespace: &str) -> Result<Vec<Table>>;
    async fn update_schema(&self, table_id: &TableId, new_schema: &Schema) -> Result<SchemaId>;
}

pub trait SnapshotRepository {
    async fn create_snapshot(&self, table_id: &TableId, manifest_list: &str) -> Result<Snapshot>;
    async fn get_snapshot(&self, snapshot_id: &SnapshotId) -> Result<Snapshot>;
    async fn list_snapshots(&self, table_id: &TableId) -> Result<Vec<Snapshot>>;
    async fn get_snapshot_at_time(&self, table_id: &TableId, timestamp: DateTime<Utc>) -> Result<Snapshot>;
}

pub trait FileRepository {
    async fn register_files(&self, snapshot_id: &SnapshotId, files: &[DataFile]) -> Result<()>;
    async fn get_files_for_snapshot(&self, snapshot_id: &SnapshotId) -> Result<Vec<DataFile>>;
    async fn get_files_for_partition(&self, table_id: &TableId, partition: &PartitionSpec) -> Result<Vec<DataFile>>;
}
```

### 3. Query Engine Integration
```rust
pub trait QueryPlanner {
    async fn plan_query(&self, sql: &str) -> Result<QueryPlan>;
    async fn execute_plan(&self, plan: &QueryPlan) -> Result<RecordBatch>;
    async fn time_travel_query(&self, sql: &str, snapshot_id: &SnapshotId) -> Result<RecordBatch>;
}
```

## Storage Layout

### Object Storage (S3/MinIO)
```
bucket/
├── warehouse/
│   └── my_namespace/
│       └── my_table/
│           ├── metadata/
│           │   ├── v1.metadata.json
│           │   ├── v2.metadata.json
│           │   └── snap-123.avro      # Manifest list
│           └── data/
│               ├── partition_1/
│               │   ├── file_001.parquet
│               │   └── file_002.parquet
│               └── partition_2/
│                   └── file_003.parquet
```

### Database Tables
- `iceberg_catalogs` - Catalog configurations
- `iceberg_namespaces` - Database/schema organization  
- `iceberg_tables` - Table registry with current pointers
- `table_schemas` - Schema evolution history
- `table_snapshots` - Snapshot history for time travel
- `manifest_lists` - File group metadata
- `data_files` - Individual file tracking with statistics
- `partition_specs` - Partitioning strategies
- `sort_orders` - Table sorting specifications

## Key Benefits

1. **Schema Evolution**: Add/remove/rename columns without rewriting data
2. **Time Travel**: `SELECT * FROM table TIMESTAMP AS OF '2024-01-01'`
3. **ACID Transactions**: Atomic commits ensure consistency
4. **Efficient Pruning**: Skip reading unnecessary files using statistics
5. **Hidden Partitioning**: Automatic partition management
6. **Compaction**: Background optimization of small files

## Implementation Strategy

1. **Phase 1**: Basic catalog with table/schema management
2. **Phase 2**: Snapshot management and time travel
3. **Phase 3**: Query planning and execution
4. **Phase 4**: Advanced features (compaction, optimization)