use std::fs::File;

extern crate viz;
use viz::{SlogJSONReader, VizWriter, TikzWriter}; 

extern crate failure;

fn plot_log(logfile: &str, outfile: &str) -> Result<(), failure::Error> {
    let logfile = File::open(logfile)?;
    let outfile = File::create(outfile)?;
    let reader = std::io::BufReader::new(logfile);
    let reader = SlogJSONReader::new(reader);
    let mut writer = TikzWriter::new(outfile);
    writer.dump_events(
        reader
            .get_events()
            .filter(|e| {
                e.annotation().as_str().starts_with("1")
            }),
    )?;
    Ok(())
}

fn compile_viz(outfile: &str) -> Result<(), failure::Error> {
    use std::process::Command;

    Command::new("pdflatex")
        .arg(outfile)
        .spawn()?
        .wait()?;

    Ok(())
}

fn main() {
    let mut args = std::env::args();
    args.next().unwrap();
    let fln = args.next().unwrap();
    let outfln = args.next().unwrap();

    plot_log(&fln, &outfln).unwrap();
    compile_viz(&outfln).unwrap();
}
