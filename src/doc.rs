use std::env;
use std::fs::File;
use std::path::PathBuf;
use std::io::prelude::*;
use std::io::BufReader;
use regex::Regex;
use toml;

#[derive(PartialEq)]
enum Code {
    Rust,
    Other,
    Doc,
}

#[derive(Clone)]
pub struct CrateInfo {
    pub name: String,
    pub license: Option<String>,
    pub bin: Option<String>,
    pub lib: Option<String>,
}

/// Given the current directory, start from there, and go up, and up, until a Cargo.toml file has
/// been found. If a Cargo.toml folder has been found, then we have found the project dir. If not,
/// nothing is found, and we return None.
pub fn project_root_dir() -> Option<PathBuf> {
    let mut currpath = env::current_dir().unwrap();

    fn _is_file(p: &PathBuf) -> bool {
        use std::fs;

        match fs::metadata(p) {
            Ok(v) => v.file_type().is_file(),
            // Errs only if not enough fs permissions, or no fs entry
            Err(..) => return false,
        }
    }

    while currpath.parent().is_some() {
        currpath.push("Cargo.toml");
        if _is_file(&currpath) {
            currpath.pop(); // found, remove toml, return project root
            return Some(currpath);
        }
        currpath.pop(); // remove toml filename
        currpath.pop(); // next dir
    }

    None
}

/// Generates readme data from `source` file
pub fn generate_readme<T: Read>(source: &mut T,
                                template: &mut Option<T>,
                                add_title: bool,
                                add_license: bool,
                                indent_headings: bool)
                                -> Result<String, String>
{
    let doc_data = extract(source, indent_headings);
    let mut readme = fold_data(doc_data);

    let crate_info = try!(get_crate_info());
    if add_license && crate_info.license.is_none() {
        return Err("There is no license in Cargo.toml".to_owned());
    }

    match template.as_mut() {
        Some(template) => process_template(template, readme, crate_info, add_title, add_license),
        None => {
            if add_title {
                readme = prepend_title(readme, &crate_info.name);
            }

            if add_license {
                readme = append_license(readme, &crate_info.license.unwrap());
            }

            Ok(readme)
        }
    }
}

/// Extracts the doc comments as a Vec of lines
///
/// Doc tests are automatically transformed into '```rust'.
/// Lines that would not show in rust docs are not returned.
fn extract<T: Read>(source: &mut T, indent_headings: bool) -> Vec<String> {
    let reader = BufReader::new(source);

    // Is this code block rust?
    let re_code_rust = Regex::new(r"^//! ```(no_run|ignore|should_panic)?$").unwrap();
    // Is this code block a language other than rust?
    let re_code_other = Regex::new(r"//! ```\w+").unwrap();

    let mut section = Code::Doc;

    reader.lines()
          .filter_map(|line| {
              let mut line = line.unwrap();
              if line.starts_with("//!") {

                  if section == Code::Doc && re_code_rust.is_match(&line) {
                      section = Code::Rust;

                      return Some("```rust".to_owned());
                  } else if section == Code::Doc && re_code_other.is_match(&line) {
                      section = Code::Other;
                  } else if section != Code::Doc && line == "//! ```" {
                      section = Code::Doc;

                      return Some("```".to_owned());
                  }

                  // If line is hidden in documentation, it is also hidden in README
                  if section == Code::Rust && line.starts_with("//! # ") {
                      return None;
                  }

                  // Remove leading '//!' before returning the line
                  if line.trim() == "//!" {
                      line = String::new();
                  } else {
                      line = line[4..].to_owned();
                      // If we should indent headings, only do this outside code blocks
                      if indent_headings && section == Code::Doc && line.starts_with("#") {
                          line.insert(0, '#');
                      }
                  }

                  Some(line)
              } else {
                  return None;
              }
          })
          .collect()
}

/// Renders the template
///
/// This is not a real template engine, it just processes a few substitutions.
fn process_template<T: Read>(template: &mut T,
                             mut readme: String,
                             crate_info: CrateInfo,
                             add_title: bool,
                             add_license: bool)
                             -> Result<String, String> {

    let mut template = try!(get_template(template));
    template = template.trim_right_matches("\n").to_owned();

    if add_title && !template.contains("{{crate}}") {
        readme = prepend_title(readme, &crate_info.name);
    } else {
        template = template.replace("{{crate}}", &crate_info.name);
    }

    if template.contains("{{license}}") && crate_info.license.is_none() {
        return Err("`{{license}}` found in template but there is no license in Cargo.toml".to_owned());
    }

    if add_license && crate_info.license.is_none() {
        return Err("There is no license in Cargo.toml".to_owned());
    }

    if add_license && !template.contains("{{license}}") {
        readme = append_license(readme, &crate_info.license.unwrap());
    } else if template.contains("{{license}}") {
        template = template.replace("{{license}}", &crate_info.license.unwrap())
    }

    if !template.contains("{{readme}}") {
        return Err("Missing `{{readme}}` in template".to_owned());
    }

    let result = template.replace("{{readme}}", &readme);
    Ok(result)
}

/// Try to get crate name and license from Cargo.toml
pub fn get_crate_info() -> Result<CrateInfo, String> {
    let current_dir = match project_root_dir() {
        Some(v) => v,
        None => return Err("Not in a rust project".into()),
    };

    let mut cargo_toml = match File::open(current_dir.join("Cargo.toml")) {
        Ok(file) => file,
        Err(_) => return Err(format!("Cargo.toml not found in '{}'",
                                     current_dir.to_string_lossy())),
    };

    let mut buf = String::new();
    match cargo_toml.read_to_string(&mut buf) {
        Err(e) => return Err(format!("{}", e)),
        Ok(_) => {}
    }

    let table = toml::Value::Table(toml::Parser::new(&buf).parse().unwrap());

    // Crate name is required, right?
    let crate_name = table.lookup("package.name").unwrap().as_str().unwrap().to_owned();
    let license = table.lookup("package.license").map(|v| v.as_str().unwrap().to_owned());
    let lib = table.lookup("lib.path").map(|v| v.as_str().unwrap().to_owned());

    let mut bin: Option<String> = None;
    match table.lookup("bin").map(|v| v.as_slice().unwrap()) {
        Some(v) => {
            for i in  0..(v.len()) {
                if let Some(p) = table.lookup(&format!("bin.{}.path", i)) {
                    bin = Some(p.as_str().unwrap().to_owned());
                }
            }
        },
        _ => {}
    };

    Ok(CrateInfo {
        name: crate_name,
        license: license,
        lib: lib,
        bin: bin,
    })
}

/// Transforms the Vec of lines into a single String
fn fold_data(data: Vec<String>) -> String {
    if data.len() < 1 {
        String::new()
    } else if data.len() < 2 {
        data[0].to_owned()
    } else {
        data[1..].into_iter().fold(data[0].to_owned(), |acc, line| format!("{}\n{}", acc, line))
    }
}

fn get_template<T: Read>(template: &mut T) -> Result<String, String> {
    let mut template_string = String::new();
    match template.read_to_string(&mut template_string) {
        Err(e) => return Err(format!("Error: {}", e)),
        _ => {}
    }

    Ok(template_string)
}

fn prepend_title(readme: String, crate_name: &str) -> String {
    let mut new_readme = format!("# {}\n\n", crate_name);
    new_readme.push_str(&readme);

    new_readme
}

fn append_license(readme: String, license: &str) -> String {
    let mut new_readme = String::new();
    new_readme.push_str(&format!("{}\n\nLicense: {}", &readme, &license));

    new_readme
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    const TEMPLATE_NO_CRATE_NO_LICENSE: &'static str = "{{readme}}";
    const TEMPLATE_CRATE_NO_LICENSE: &'static str = "# {{crate}}\n\n{{readme}}";
    const TEMPLATE_NO_CRATE_LICENSE: &'static str = "{{readme}}\n\nLicense: {{license}}";
    const TEMPLATE_CRATE_LICENSE: &'static str = "# {{crate}}\n\n{{readme}}\n\nLicense: {{license}}";

    const INPUT: &'static str =
r#"//! first line
//! ```
//! let rust_code = "will show";
//! # let binding = "won't show";
//! ```
//! # heading
//! ```no_run
//! let no_run = true;
//! ```
//! ```ignore
//! let ignore = true;
//! ```
//! ```should_panic
//! let should_panic = true;
//! ```
//! # heading
//! ```C
//! int i = 0; // no rust code
//! ```
use std::any::Any;

fn main() {}"#;

    #[test]
    fn extract_indent_headings() {
        let expected: Vec<_> =
r#"first line
```rust
let rust_code = "will show";
```
## heading
```rust
let no_run = true;
```
```rust
let ignore = true;
```
```rust
let should_panic = true;
```
## heading
```C
int i = 0; // no rust code
```"#.lines().collect();

        let mut cursor = Cursor::new(INPUT.as_bytes());
        let doc_data = super::extract(&mut cursor, true);

        assert_eq!(doc_data, expected);
    }

    #[test]
    fn extract_no_indent_headings() {
        let expected: Vec<_> =
r#"first line
```rust
let rust_code = "will show";
```
# heading
```rust
let no_run = true;
```
```rust
let ignore = true;
```
```rust
let should_panic = true;
```
# heading
```C
int i = 0; // no rust code
```"#.lines().collect();

        let mut cursor = Cursor::new(INPUT.as_bytes());
        let doc_data = super::extract(&mut cursor, false);

        assert_eq!(doc_data, expected);
    }

    #[test]
    fn fold_data_empty_input() {
        let input: Vec<String> = vec![];

        let result = super::fold_data(input);

        assert!(result.is_empty());
    }

    #[test]
    fn fold_data_single_line() {
        let line = "# single line";
        let input: Vec<String> = vec![line.to_owned()];

        let result = super::fold_data(input);

        assert_eq!(line, result);
    }

    #[test]
    fn fold_data_multiple_lines() {
        let input: Vec<String> = vec![
            "# first line".to_owned(),
            "second line".to_owned(),
            "third line".to_owned(),
        ];

        let result = super::fold_data(input);

        assert_eq!("# first line\nsecond line\nthird line", result);
    }

    macro_rules! test_process_template {
        ( $name:ident,
          $template:ident,
          input => $input:expr,
          license => $license:expr,
          add_crate_name => $with_crate:expr,
          add_license => $with_license:expr,
          expected => $expected:expr) =>
        {
            #[test]
            fn $name() {
                let input = $input;
                let mut template = Cursor::new($template.as_bytes());

                let crate_info = super::CrateInfo {
                    name: "my_crate".into(),
                    license: $license,
                    lib: None,
                    bin: None,
                };

                let result = super::process_template(&mut template,
                                            input.into(),
                                            crate_info.clone(),
                                            $with_crate,
                                            $with_license).unwrap();
                assert_eq!($expected, result);
            }
        };

        ( $name:ident,
          $template:ident,
          input => $input:expr,
          license => $license:expr,
          add_crate_name => $with_crate:expr,
          add_license => $with_license:expr,
          panic => $panic:expr) =>
        {
            #[test]
            #[should_panic(expected = $panic)]
            fn $name() {
                let input = $input;
                let mut template = Cursor::new($template.as_bytes());

                let crate_info = super::CrateInfo {
                    name: "my_crate".into(),
                    license: $license,
                    lib: None,
                    bin: None
                };

                super::process_template(&mut template,
                                        input.into(),
                                        crate_info.clone(),
                                        $with_crate,
                                        $with_license).unwrap();
            }
        }
    }

    // TEMPLATE_NO_CRATE_NO_LICENSE
    test_process_template!(
        process_template_no_crate_no_license_with_license_prepend_crate_append_license,
        TEMPLATE_NO_CRATE_NO_LICENSE,
        input => "# documentation",
        license => Some("MIT".to_owned()),
        add_crate_name => true,
        add_license => true,
        expected => "# my_crate\n\n# documentation\n\nLicense: MIT"
    );

    test_process_template!(
        process_template_no_crate_no_license_without_license_prepend_crate_append_license,
        TEMPLATE_NO_CRATE_NO_LICENSE,
        input => "# documentation",
        license => None,
        add_crate_name => true,
        add_license => true,
        panic => "There is no license in Cargo.toml"
    );

    test_process_template!(
        process_template_no_crate_no_license_with_license_prepend_crate,
        TEMPLATE_NO_CRATE_NO_LICENSE,
        input => "# documentation",
        license => Some("MIT".to_owned()),
        add_crate_name => true,
        add_license => false,
        expected => "# my_crate\n\n# documentation"
    );

    test_process_template!(
        process_template_no_crate_no_license_without_license_prepend_crate,
        TEMPLATE_NO_CRATE_NO_LICENSE,
        input => "# documentation",
        license => None,
        add_crate_name => true,
        add_license => false,
        expected => "# my_crate\n\n# documentation"
    );

    test_process_template!(
        process_template_no_crate_no_license_with_license_append_license,
        TEMPLATE_NO_CRATE_NO_LICENSE,
        input => "# documentation",
        license => Some("MIT".to_owned()),
        add_crate_name => false,
        add_license => true,
        expected => "# documentation\n\nLicense: MIT"
    );

    test_process_template!(
        process_template_no_crate_no_license_without_license_append_license,
        TEMPLATE_NO_CRATE_NO_LICENSE,
        input => "# documentation",
        license => None,
        add_crate_name => false,
        add_license => true,
        panic => "There is no license in Cargo.toml"
    );

    test_process_template!(
        process_template_no_crate_no_license_with_license,
        TEMPLATE_NO_CRATE_NO_LICENSE,
        input => "# documentation",
        license => Some("MIT".to_owned()),
        add_crate_name => false,
        add_license => false,
        expected => "# documentation"
    );

    test_process_template!(
        process_template_no_crate_no_license_without_license,
        TEMPLATE_NO_CRATE_NO_LICENSE,
        input => "# documentation",
        license => None,
        add_crate_name => false,
        add_license => false,
        expected => "# documentation"
    );

    // TEMPLATE_CRATE_NO_LICENSE
    test_process_template!(
        process_template_crate_no_license_with_license_prepend_crate_append_license,
        TEMPLATE_CRATE_NO_LICENSE,
        input => "# documentation",
        license => Some("MIT".to_owned()),
        add_crate_name => true,
        add_license => true,
        expected => "# my_crate\n\n# documentation\n\nLicense: MIT"
    );

    test_process_template!(
        process_template_crate_no_license_without_license_prepend_crate_append_license,
        TEMPLATE_CRATE_NO_LICENSE,
        input => "# documentation",
        license => None,
        add_crate_name => true,
        add_license => true,
        panic => "There is no license in Cargo.toml"
    );

    test_process_template!(
        process_template_crate_no_license_with_license_prepend_crate,
        TEMPLATE_CRATE_NO_LICENSE,
        input => "# documentation",
        license => Some("MIT".to_owned()),
        add_crate_name => true,
        add_license => false,
        expected => "# my_crate\n\n# documentation"
    );

    test_process_template!(
        process_template_crate_no_license_without_license_prepend_crate,
        TEMPLATE_CRATE_NO_LICENSE,
        input => "# documentation",
        license => None,
        add_crate_name => true,
        add_license => false,
        expected => "# my_crate\n\n# documentation"
    );

    test_process_template!(
        process_template_crate_no_license_with_license_append_license,
        TEMPLATE_CRATE_NO_LICENSE,
        input => "# documentation",
        license => Some("MIT".to_owned()),
        add_crate_name => false,
        add_license => true,
        expected => "# my_crate\n\n# documentation\n\nLicense: MIT"
    );

    test_process_template!(
        process_template_crate_no_license_without_license_append_license,
        TEMPLATE_CRATE_NO_LICENSE,
        input => "# documentation",
        license => None,
        add_crate_name => false,
        add_license => true,
        panic => "There is no license in Cargo.toml"
    );

    test_process_template!(
        process_template_crate_no_license_with_license,
        TEMPLATE_CRATE_NO_LICENSE,
        input => "# documentation",
        license => Some("MIT".to_owned()),
        add_crate_name => false,
        add_license => false,
        expected => "# my_crate\n\n# documentation"
    );

    test_process_template!(
        process_template_crate_no_license_without_license,
        TEMPLATE_CRATE_NO_LICENSE,
        input => "# documentation",
        license => None,
        add_crate_name => false,
        add_license => false,
        expected => "# my_crate\n\n# documentation"
    );

    // TEMPLATE_NO_CRATE_LICENSE
    test_process_template!(
        process_template_no_crate_license_with_license_prepend_crate_append_license,
        TEMPLATE_NO_CRATE_LICENSE,
        input => "# documentation",
        license => Some("MIT".to_owned()),
        add_crate_name => true,
        add_license => true,
        expected => "# my_crate\n\n# documentation\n\nLicense: MIT"
    );

    test_process_template!(
        process_template_no_crate_license_without_license_prepend_crate_append_license,
        TEMPLATE_NO_CRATE_LICENSE,
        input => "# documentation",
        license => None,
        add_crate_name => true,
        add_license => true,
        panic => "`{{license}}` found in template but there is no license in Cargo.toml"
    );

    test_process_template!(
        process_template_no_crate_license_with_license_prepend_crate,
        TEMPLATE_NO_CRATE_LICENSE,
        input => "# documentation",
        license => Some("MIT".to_owned()),
        add_crate_name => true,
        add_license => false,
        expected => "# my_crate\n\n# documentation\n\nLicense: MIT"
    );

    test_process_template!(
        process_template_no_crate_license_without_license_prepend_crate,
        TEMPLATE_NO_CRATE_LICENSE,
        input => "# documentation",
        license => None,
        add_crate_name => true,
        add_license => false,
        panic => "`{{license}}` found in template but there is no license in Cargo.toml"
    );

    test_process_template!(
        process_template_no_crate_license_with_license_append_license,
        TEMPLATE_NO_CRATE_LICENSE,
        input => "# documentation",
        license => Some("MIT".to_owned()),
        add_crate_name => false,
        add_license => true,
        expected => "# documentation\n\nLicense: MIT"
    );

    test_process_template!(
        process_template_no_crate_license_without_license_append_license,
        TEMPLATE_NO_CRATE_LICENSE,
        input => "# documentation",
        license => None,
        add_crate_name => true,
        add_license => false,
        panic => "`{{license}}` found in template but there is no license in Cargo.toml"
    );

    test_process_template!(
        process_template_no_crate_license_with_license,
        TEMPLATE_NO_CRATE_LICENSE,
        input => "# documentation",
        license => Some("MIT".to_owned()),
        add_crate_name => false,
        add_license => false,
        expected => "# documentation\n\nLicense: MIT"
    );

    test_process_template!(
        process_template_no_crate_license_without_license,
        TEMPLATE_NO_CRATE_LICENSE,
        input => "# documentation",
        license => None,
        add_crate_name => false,
        add_license => false,
        panic => "`{{license}}` found in template but there is no license in Cargo.toml"
    );

    // TEMPLATE_CRATE_LICENSE
    test_process_template!(
        process_template_crate_license_with_license_prepend_crate_append_license,
        TEMPLATE_CRATE_LICENSE,
        input => "# documentation",
        license => Some("MIT".to_owned()),
        add_crate_name => true,
        add_license => true,
        expected => "# my_crate\n\n# documentation\n\nLicense: MIT"
    );

    test_process_template!(
        process_template_crate_license_without_license_prepend_crate_append_license,
        TEMPLATE_CRATE_LICENSE,
        input => "# documentation",
        license => None,
        add_crate_name => true,
        add_license => true,
        panic => "`{{license}}` found in template but there is no license in Cargo.toml"
    );

    test_process_template!(
        process_template_crate_license_with_license_prepend_crate,
        TEMPLATE_CRATE_LICENSE,
        input => "# documentation",
        license => Some("MIT".to_owned()),
        add_crate_name => true,
        add_license => false,
        expected => "# my_crate\n\n# documentation\n\nLicense: MIT"
    );

    test_process_template!(
        process_template_crate_license_without_license_prepend_crate,
        TEMPLATE_CRATE_LICENSE,
        input => "# documentation",
        license => None,
        add_crate_name => true,
        add_license => false,
        panic => "`{{license}}` found in template but there is no license in Cargo.toml"
    );

    test_process_template!(
        process_template_crate_license_with_license_append_license,
        TEMPLATE_CRATE_LICENSE,
        input => "# documentation",
        license => Some("MIT".to_owned()),
        add_crate_name => false,
        add_license => true,
        expected => "# my_crate\n\n# documentation\n\nLicense: MIT"
    );

    test_process_template!(
        process_template_crate_license_without_license_append_license,
        TEMPLATE_CRATE_LICENSE,
        input => "# documentation",
        license => None,
        add_crate_name => false,
        add_license => true,
        panic => "`{{license}}` found in template but there is no license in Cargo.toml"
    );

    test_process_template!(
        process_template_crate_license_with_license,
        TEMPLATE_CRATE_LICENSE,
        input => "# documentation",
        license => Some("MIT".to_owned()),
        add_crate_name => false,
        add_license => false,
        expected => "# my_crate\n\n# documentation\n\nLicense: MIT"
    );

    test_process_template!(
        process_template_crate_license_without_license,
        TEMPLATE_CRATE_LICENSE,
        input => "# documentation",
        license => None,
        add_crate_name => false,
        add_license => false,
        panic => "`{{license}}` found in template but there is no license in Cargo.toml"
    );
}
