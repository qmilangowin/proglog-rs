use proglog_rs::storage::index::Index;
use proglog_rs::storage::store::Store;
use tempfile::TempDir;

#[test]
fn test_store_index_coordination() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let store_path = temp_dir.path().join("test.log");
    let index_path = temp_dir.path().join("test.idx");

    let records = [
        "Hello, World!",
        "This is record 2",
        "Short",
        "This is a much longer record with more text to see variable sizing",
        "Final record",
    ];

    let mut positions = Vec::new();

    {
        let mut store = Store::new(&store_path)?;
        let mut index = Index::new(&index_path)?;

        for (offset, record) in records.iter().enumerate() {
            let data = record.as_bytes();
            let (position, _bytes_written) = store.append(data)?;
            index.write(offset as u64, position)?;
            positions.push(position);
        }

        assert_eq!(index.len(), records.len().try_into().unwrap());
    }

    {
        let store = Store::new(&store_path)?;
        let index = Index::new(&index_path)?;

        for (i, &expected_pos) in positions.iter().enumerate() {
            let position = index.read(i as u64)?;
            assert_eq!(position, expected_pos);

            let (data, _) = store.read(position)?;
            assert_eq!(data, records[i].as_bytes());
        }
    }

    Ok(())
}

#[test]
fn test_random_access_via_index() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let store_path = temp_dir.path().join("test.log");
    let index_path = temp_dir.path().join("test.idx");

    let records = ["First", "Second", "Third", "Fourth", "Fifth"];

    {
        let mut store = Store::new(&store_path)?;
        let mut index = Index::new(&index_path)?;

        for (offset, record) in records.iter().enumerate() {
            let (position, _) = store.append(record.as_bytes())?;
            index.write(offset as u64, position)?;
        }
    }

    {
        let store = Store::new(&store_path)?;
        let index = Index::new(&index_path)?;

        let access_pattern = [2, 0, 4, 1, 3];
        for &offset in &access_pattern {
            let position = index.read(offset)?;
            let (data, _) = store.read(position)?;
            assert_eq!(data, records[offset as usize].as_bytes());
        }
    }

    Ok(())
}

#[test]
fn test_storage_overhead_analysis() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let store_path = temp_dir.path().join("test.log");
    let index_path = temp_dir.path().join("test.idx");

    let mut store = Store::new(&store_path)?;
    let mut index = Index::new(&index_path)?;

    let num_records = 100;
    let record = "Test record data";

    for i in 0..num_records {
        let (position, _) = store.append(record.as_bytes())?;
        index.write(i, position)?;
    }

    let index_size = index.size();
    let bytes_per_entry = index_size / num_records;

    assert_eq!(
        bytes_per_entry, 16,
        "Index entry should be 16 bytes (8 bytes offset + 8 bytes position)"
    );

    let store_size = store.size();
    let expected_store_size = num_records * (8 + record.len() as u64);
    assert_eq!(
        store_size, expected_store_size,
        "Store size should match expected"
    );

    Ok(())
}
