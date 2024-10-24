
build:
    cargo build -p  ferritin-core -p  ferritin-pymol  -p ferritin-bevy


# convert all PSEs to msvj folders
convert: build
    for file in docs/examples/*.pse; do \
        ./target/debug/ferritin-pymol --psefile "$file" --outputdir "${file%.*}"; \
    done


docs: convert
    # generate and copy rust docs
    cargo doc --workspace --no-deps
    cp -r target/doc/  docs/
    # quarto
    quarto render docs

serve: docs
    quarto preview docs

clean:
    cargo clean -p  ferritin-core -p  ferritin-pymol  -p ferritin-bevy
    cargo clean --doc
    rm -rf docs/doc/
    rm -rf docs/examples/example


# cargo install cargo-edit
upgrade:
    cargo upgrade --all
