fn main() -> anyhow::Result<()> {
    respin_tools::support::init_signals()?;
    respin_tools::jev::main(std::env::args().skip(1).collect())
}
