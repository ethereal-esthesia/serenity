#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CommonRunConfig {
    pub debug: bool,
    pub naive_mod_detect: bool,
    pub disable_global_input: bool,
    pub screenshot_path: Option<String>,
}

pub fn parse_common_args_from(
    args: impl IntoIterator<Item = String>,
) -> Result<CommonRunConfig, Box<dyn std::error::Error>> {
    let mut config = CommonRunConfig::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--debug" => config.debug = true,
            "--naive-mod-detect" => config.naive_mod_detect = true,
            "--disable-global-input" => config.disable_global_input = true,
            "--screenshot" => {
                let path = args.next().ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "--screenshot requires a file path",
                    )
                })?;
                config.screenshot_path = Some(path);
            }
            _ => {}
        }
    }
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::{CommonRunConfig, parse_common_args_from};

    #[test]
    fn parse_args_defaults() {
        let cfg = parse_common_args_from(Vec::new()).expect("default parse should succeed");
        assert_eq!(cfg, CommonRunConfig::default());
    }

    #[test]
    fn parse_args_debug_and_screenshot() {
        let cfg = parse_common_args_from(vec![
            "--debug".to_string(),
            "--screenshot".to_string(),
            "/tmp/test.ppm".to_string(),
        ])
        .expect("parse should succeed");
        assert_eq!(
            cfg,
            CommonRunConfig {
                debug: true,
                naive_mod_detect: false,
                disable_global_input: false,
                screenshot_path: Some("/tmp/test.ppm".to_string()),
            }
        );
    }

    #[test]
    fn parse_args_screenshot_only() {
        let cfg = parse_common_args_from(vec![
            "--screenshot".to_string(),
            "/tmp/noise.ppm".to_string(),
        ])
        .expect("parse should succeed");
        assert_eq!(
            cfg,
            CommonRunConfig {
                debug: false,
                naive_mod_detect: false,
                disable_global_input: false,
                screenshot_path: Some("/tmp/noise.ppm".to_string()),
            }
        );
    }

    #[test]
    fn parse_args_naive_mod_detect() {
        let cfg = parse_common_args_from(vec!["--naive-mod-detect".to_string()])
            .expect("parse should succeed");
        assert_eq!(
            cfg,
            CommonRunConfig {
                debug: false,
                naive_mod_detect: true,
                disable_global_input: false,
                screenshot_path: None,
            }
        );
    }

    #[test]
    fn parse_args_disable_global_input() {
        let cfg = parse_common_args_from(vec!["--disable-global-input".to_string()])
            .expect("parse should succeed");
        assert_eq!(
            cfg,
            CommonRunConfig {
                debug: false,
                naive_mod_detect: false,
                disable_global_input: true,
                screenshot_path: None,
            }
        );
    }

    #[test]
    fn parse_args_rejects_missing_screenshot_path() {
        let err =
            parse_common_args_from(vec!["--screenshot".to_string()]).expect_err("missing path should fail");
        assert!(err.to_string().contains("requires a file path"));
    }
}
