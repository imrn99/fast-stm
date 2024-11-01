# Generate doc

cargo +nightly doc --all --no-deps --all-features

# Add index files

cp docs/index.html target/doc
cp docs/index.css target/doc
cp docs/theme.js target/doc
