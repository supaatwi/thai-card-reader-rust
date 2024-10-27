# Thai Card Reader in Rust

This project provides a Rust implementation for reading information from Thai national ID cards. It supports extracting data via card readers compliant with Thai card standards.

## Features

- Extract essential information from Thai national ID cards.
- Open websocket port ```9982``` prepare send data other client
- Uses Rust for high performance and safety.
- Designed for compatibility with commonly used card readers in Thailand.

## Prerequisites

- Rust (version 1.70+)
- Compatible Thai card reader
- Libraries required by the card reader drivers (please refer to your device documentation for specific dependencies)

## Installation

1. Clone the repository:
```bash
   git clone https://github.com/supaatwi/thai-card-reader-rust.git
   cd thai-card-reader-rust
```
2. Install Rust dependencies:
```bash
  cargo build
```

## Usage
1. Connect your Thai ID card reader to the computer.
2. Run the application to start reading data:
```bash
   cargo run
```
