fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let command = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("Usage: respin-tools COMMAND [ARGS]"))?;
    respin_tools::dispatch(&command, args.collect())
}
