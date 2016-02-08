//! Generate README.md from doc comments.
//!
//! # Installation
//!
//! Just clone this repository, run `cargo build --release` and add `target/release/cargo-readme`
//! somewhere in your path.
//!
//! # About
//!
//! This cargo subcommand will extract documentation from your crate's doc comments
//! that you can use to populate its README.md.
//!
//! For example, if your crate has the following doc comments at `lib.rs`
//!
//! ```rust
//! //! This is my awesome crate
//! //!
//! //! Here goes some other description of what it is and what is does
//! //!
//! //! # Examples
//! //! ```
//! //! fn sum2(n1: i32, n2: i32) -> i32 {
//! //!   n1 + n2
//! //! }
//! //! # assert_eq!(4, sum2(2, 2));
//! //! ```
//! ```
//!
//! you may want to use it as your `README.md` content (without rust's doc comments specific stuff, obviously)
//! so you don't have to maintain the same documentation in the different places.
//!
//! Using `cargo-readme`, you write the documentation as doc comments, fill README.md with it and
//! you can be sure that the examples are correct.
//!
//! The result would look like:
//!
//!     # crate-name
//!
//!     This is my awesome crate
//!
//!     Here goes some other description of what it is and what is does
//!
//!     ## Examples
//!     ```rust
//!     fn sum2(n1: i32, n2: i32) -> i32 {
//!       n1 + n2
//!     }
//!     ```
//!
//! You may have noticed that `# Examples` became `## Examples`. This is intentional (and can be disabled)
//! so in README.md the first heading can be your crate name.
//!
//! Also, the crate name was automatically added (can be disabled too). It is read
//! from `Cargo.toml` so you just need to have them there. License can be read from
//! `Cargo.toml`, but it's opt-in.
//!
//! If you have additional information that does not fit in doc comments, you can use
//! a template. To do so, just create a file called `README.tpl` in the same directory
//! as `Cargo.toml` with the following content
//!
//!     Your crate's badges here
//!
//!     {{readme}}
//!
//!     Some additional info here
//!
//! The output will look like this
//!
//!     # crate-name
//!
//!     Your crate's badges here
//!
//!     This is my awesome crate
//!
//!     Here goes some other description of what it is and what is does
//!
//!     ## Examples
//!     ```rust
//!     fn sum2(n1: i32, n2: i32) -> i32 {
//!       n1 + n2
//!     }
//!     ```
//!
//!     Some additional info here
//!
//! You can override the displaying of your crate's name and license using `{{crate}}`
//! and `{{license}}`.

#[macro_use]
extern crate clap;
extern crate toml;
extern crate regex;

mod doc;

use std::io::{Write, ErrorKind};
use std::fs::{File};
use clap::{Arg, ArgMatches, App, AppSettings, SubCommand};
use doc::project_root_dir;

const DEFAULT_TEMPLATE: &'static str = "README.tpl";

fn main() {
    let matches = App::new("cargo-readme")
        .version(&*format!("v{}", crate_version!()))
        // We have to lie about our binary name since this will be a third party
        // subcommand for cargo but we want usage strings to generated properly
        .bin_name("cargo")
        // Global version uses the version we supplied (Cargo.toml) for all subcommands as well
        .settings(&[AppSettings::GlobalVersion, AppSettings::SubcommandRequired])
        // We use a subcommand because everything parsed after `cargo` is sent to the third party
        // plugin which will then be interpreted as a subcommand/positional arg by clap
        .subcommand(SubCommand::with_name("readme")
            .author("Livio Ribeiro <livioribeiro@outlook.com>")
            .about("Generate README.md from doc comments")
            .arg(Arg::with_name("INPUT")
                .short("i")
                .long("input")
                .takes_value(true)
                .help("File to read from. If not provided, will take 'src/lib.rs' or 'src/main.rs'."))
            .arg(Arg::with_name("OUTPUT")
                .short("o")
                .long("output")
                .takes_value(true)
                .help("File to write to. If not provided, will output to the console."))
            .arg(Arg::with_name("TEMPLATE")
                .short("t")
                .long("template")
                .takes_value(true)
                .conflicts_with("NO_TEMPLATE")
                .help("Template used to render the output. Defaults to 'README.tpl'.{n}\
                       If the default template is not found, the processed docstring will be used.{n}"))
            .arg(Arg::with_name("NO_TITLE")
                .long("no-title")
                .help("Do not prepend title line. By default, the title ('# crate-name'){n}\
                       is prepended to the output. However, if a template is used and{n}\
                       it contains the tag '{{crate}}', the template takes precedence{n}\
                       and the title is not output.{n}"))
            .arg(Arg::with_name("APPEND_LICENSE")
                .long("append-license")
                .help("Append license line. If a template is used and{n}\
                       it contains the tag '{{license}}', the template takes precedence{n}\
                       and the license is not output.{n}"))
            .arg(Arg::with_name("NO_TEMPLATE")
                .long("no-template")
                .help("Ignore template file when generating README.{n}\
                       Only useful to ignore default template README.tpl.{n}"))
            .arg(Arg::with_name("NO_INDENT_HEADINGS")
                .long("no-indent-headings")
                .help("Do not add an extra level to headings.{n}\
                       By default, '#' headings become '##',{n}\
                       so the first '#' can be your crate name.{n}\
                       Use this option to prevent this behavior.{n}")))
        .get_matches();

    if let Some(m) = matches.subcommand_matches("readme") {
        execute(m);
    }
}

fn execute(m: &ArgMatches) {
    let current_dir = match project_root_dir() {
        Some(v) => v,
        None => {
            panic!("This doesn't look like a Rust/Cargo project");
        },
    };

    let input = m.value_of("INPUT");
    let output = m.value_of("OUTPUT");
    let template = m.value_of("TEMPLATE");
    let add_title = !m.is_present("NO_TITLE");
    let add_license = m.is_present("APPEND_LICENSE");
    let no_template = m.is_present("NO_TEMPLATE");
    let indent_headings = !m.is_present("NO_INDENT_HEADINGS");

    let mut source = match input {
        Some(input) => {
            let input = current_dir.join(input);
            File::open(&input).expect(
                &format!("Could not open file '{}'", input.to_string_lossy())
            )
        }
        None => {
            let lib_rs = current_dir.join("src/lib.rs");
            let main_rs = current_dir.join("src/main.rs");
            File::open(lib_rs).or(File::open(main_rs))
                .or_else(|_| {
                    let info = doc::get_crate_info().unwrap();
                    File::open(info.lib.or(info.bin).unwrap_or(String::new()))
                })
                .expect("No entrypoint found")
        }
    };

    let mut dest = output.and_then(|output| {
        let output = current_dir.join(output);
        let file = File::create(&output).expect(
            &format!("Could not create output file '{}'", output.to_string_lossy())
        );

        Some(file)
    });

    let mut template_file: Option<File>;

    if no_template {
        template_file = None;
    } else {
        template_file = template.map(|template| {
            let template = current_dir.join(template);
            let file = File::open(&template).expect(
                &format!("Could not open template file '{}'", template.to_string_lossy())
            );
            file
        }).or_else(|| { // try read default template
            let template = current_dir.join(DEFAULT_TEMPLATE);
            let file = match File::open(&template) {
                Ok(file) => file,
                Err(ref e) if e.kind() == ErrorKind::NotFound => return None,
                e => e.expect(&format!("Could not open template file '{}'", DEFAULT_TEMPLATE)),
            };
            Some(file)
        });
    }

    let doc_string = match doc::generate_readme(&mut source,
                                                &mut template_file,
                                                add_title,
                                                add_license,
                                                indent_headings)
    {
        Ok(doc) => doc,
        Err(e) => {
            println!("Error: {}", e);
            return;
        },
    };

    match dest.as_mut() {
        Some(dest) => {
            dest.write_all(doc_string.as_bytes()).expect(
                &format!("Could not write to file '{}'", output.unwrap())
            );

            // Append new line at end of file to match behavior of `cargo readme > README.md`
            dest.write(b"\n").ok();
        }
        None => println!("{}", doc_string),
    }
}
