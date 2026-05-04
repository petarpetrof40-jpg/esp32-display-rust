fn main() {
    // Output environment variables consumed by esp-idf-sys / embuild so that
    // the ESP-IDF build system can locate the IDF, the toolchain and the target
    // SDK configuration.  This call must be present in every ESP-IDF project.
    embuild::espidf::sysenv::output();
}
