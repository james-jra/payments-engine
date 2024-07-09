use std::env;
use std::error::Error;
use std::path::Path;

fn main() -> Result<(), Box<dyn Error>> {
    let infile = env::args().nth(1).expect("No input CSV file given.");
    let reader = std::fs::File::open(Path::new(&infile))?;
    let writer = std::io::stdout();
    payments_engine::run_with_csv(reader, writer)?;
    Ok(())
}
