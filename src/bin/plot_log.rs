extern crate viz;
extern crate failure;

fn main() {
    let mut args = std::env::args();
    args.next().unwrap();
    let fln = args.next().unwrap();
    let flow = args.next().unwrap();
    viz::plot_log(&fln, flow.parse().unwrap()).unwrap();
}
