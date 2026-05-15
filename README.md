# x86_64_LEAP
x86_64_LEAP: A bare-metal #![no_std] Rust engine for Intel i7 platforms. It uses nanosecond-level MSR/RAPL modulation and predictive cache-line de-allocation (`clflushopt`) to prevent Vdroop, eliminate static leakage current, and stop thermal throttling at Ring 0—maximizing sustained performance and battery life.
