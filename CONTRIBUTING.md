# Contributing

Thanks for your interest in improving the thermal controller!

This project targets the **Radxa Cubie A7A** (Allwinner A733). The code and
tests are written for that board, and we have only verified it there. If you
test on other hardware, please tell us so we can confirm compatibility.

## Getting started

1. Fork the repo and clone your fork:

   ```sh
   git clone https://github.com/Redcat121/Radxa-Cubie-A7A-thermal-controller.git
   cd Radxa-Cubie-A7A-thermal-controller
   ```

2. Install Rust:

   ```sh
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   source $HOME/.cargo/env
   ```

3. Run the tests (no hardware needed):

   ```sh
   cargo test
   ```

## How to report a bug

Please file an issue on GitHub with:

- The board and firmware version.
- What you expected vs. what actually happened.
- Steps to reproduce.

If you can, include the output of the status banner so we can see the
temperature and fan readings.

## How to propose a feature

Open an issue describing the feature and why it helps. We'll discuss the
design before any implementation.

## Making changes

- Keep the project **A7A-focused**. Don't add references to other boards unless
  you've actually tested them.
- Update the README when you change behavior or configuration.
- Keep the code and tests passing (`cargo test`, `cargo build --release`).
- Prefer small, focused pull requests.

## Pull request checklist

- [ ] Tests pass (`cargo test`).
- [ ] Build succeeds (`cargo build --release`).
- [ ] No references to untested boards.
- [ ] README updated if behavior changed.
- [ ] Commit message follows conventional commits (e.g. `fix: ...`, `feat: ...`).

## License

By contributing, you agree that your contributions will be licensed under the
[MIT License](LICENSE) included in this repository.
