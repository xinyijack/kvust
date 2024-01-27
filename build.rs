fn main() {
    let mut config = prost_build::Config::new();
    config.bytes(&["."]);
    config.type_attribute(".", "#[derive(PartialOrd)]");
    config
        .out_dir("src/pb")
        .compile_protos(&["src/abi.proto"], &["src/"])
        .unwrap();
    // Command::new("cargo")
    //     .args(&["fmt", "--", "src/*.rs"])
    //     .status()
    //     .expect("cargo fmt failed");
}