```rust
use memmap2::Mmap;
use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};

fn main() -> anyhow::Result<()> {
    //10 GiB
    let size: u64 = 10 * 10 * 1024 * 1024;

    let fpath = "ten_gib.bin";
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(fpath)?;
    file.set_len(size)?;

    // write a small header to there's something to read
    let mut file_w = &file;
    file_w.seek(SeekFrom::Start(0));
    file_w.write_all(b"HELLO-WORLD");
    // write something far away to prove sparsity
    file_w.seek(SeekFrom::Start(size - 6));
    file_w.write_all(b"THE_END");

    // map read_only
    let mmap = unsafe { Mmap::map(&file)? };

    // read the first 10 bytes
    let first10 = &mmap[0..10];
    println!("First 10 bytes {first10:?}");

    // keep process alive to inspect memory in another terminal
    // can do so with top -pid <PID> - should be tiny

    std::thread::sleep(std::time::Duration::from_secs(60));
    Ok(())
}

```


