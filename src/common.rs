#![allow(clippy::too_many_arguments)]

use std::{
    ffi::OsStr,
    fs::{self, copy, create_dir_all, File, remove_dir_all, remove_file},
    io::{self, BufRead, Cursor, Read, Write},
    path::PathBuf,
    str::FromStr,
    sync::Mutex,
};

use anyhow::{anyhow, Error, Result};
use cosmic::widget;
use image::io::Reader as ImageReader;
use rand::{Rng, thread_rng};
use reqwest::Client;
use scraper::{Html, Selector};
use url::Url;
use usvg::fontdb;
use walkdir::WalkDir;

use crate::{
    icon_cache::IconCache,
    iconpicker,
    supported_browsers::{flatpak_browsers, native_browsers},
};

lazy_static::lazy_static! {
    static ref ICON_CACHE: Mutex<IconCache> = Mutex::new(IconCache::new());
}

pub fn icon_cache_get(name: &'static str, size: u16) -> widget::icon::Icon {
    let mut icon_cache = ICON_CACHE.lock().unwrap();
    icon_cache.get(name, size)
}

pub fn url_valid(url: &str) -> bool {
    Url::parse(url).is_ok()
}

pub fn is_svg(path: &str) -> bool {
    !url_valid(path) && PathBuf::from_str(path).unwrap().extension() == Some(OsStr::new("svg"))
}

pub fn home_dir() -> PathBuf {
    let home = std::env::var("HOME");

    if let Ok(path) = home {
        return PathBuf::from_str(&path).unwrap();
    }

    PathBuf::new()
}

pub fn desktop_filepath(filename: &str) -> PathBuf {
    let mut home = home_dir();
    home.push(".local/share/applications");

    home.join(filename)
}

pub fn icons_location() -> PathBuf {
    match std::env::var("FLATPAK_ID") {
        Ok(_) => {
            let mut icons_dir = home_dir();
            icons_dir.push(".var/app/io.github.elevenhsoft.WebApps/data/icons");
            icons_dir
        }
        Err(_) => {
            let mut test_path = home_dir();
            test_path.push(".local/share/icons");
            test_path
        }
    }
}

#[derive(Debug, Clone)]
pub struct WebAppLauncher {
    pub path: PathBuf,
    pub codename: String,
    pub web_browser: Browser,
    pub name: String,
    pub icon: String,
    pub is_valid: bool,
    pub exec: String,
    pub args: Vec<String>,
    pub category: String,
    pub url: String,
    pub custom_parameters: String,
    pub isolate_profile: bool,
    pub navbar: bool,
    pub is_incognito: bool,
}

impl WebAppLauncher {
    pub fn new(
        name: String,
        codename: Option<String>,
        url: String,
        icon: String,
        category: String,
        browser: Browser,
        custom_parameters: String,
        isolated: bool,
        navbar: bool,
        privatewindow: bool,
    ) -> Self {
        let codename = if let Some(codename) = codename {
            codename
        } else {
            let random_code: u16 = thread_rng().gen_range(1000..10000);
            format!("{}{}", name.replace(' ', ""), random_code)
        };
        let filename = format!("webapp-{}.desktop", codename);
        let path = desktop_filepath(&filename);
        let web_browser = browser;
        let exec = web_browser.exec.clone();
        let args = Vec::new();
        let isolate_profile = isolated;
        let is_incognito = privatewindow;

        let is_valid =
            !name.is_empty() && !icon.is_empty() && url_valid(&url) && web_browser.is_installed();

        Self {
            path,
            codename,
            web_browser,
            name,
            icon,
            is_valid,
            exec,
            args,
            category,
            url,
            custom_parameters,
            isolate_profile,
            navbar,
            is_incognito,
        }
    }

    pub fn read(path: PathBuf, codename: String) -> Result<WebAppLauncher, Error> {
        let file = File::open(&path)?;
        let mut browser_name = String::new();
        let mut name = String::new();
        let mut icon = String::new();
        let mut is_valid = false;
        let mut exec = String::new();
        let mut args = Vec::new();
        let mut category = String::new();
        let mut url = String::new();
        let mut custom_parameters = String::new();
        let mut isolate_profile = false;
        let mut navbar = false;
        let mut is_incognito = false;

        let reader = io::BufReader::new(file);

        let mut is_webapp = false;

        for line_result in reader.lines() {
            match line_result {
                Ok(line) => {
                    if line.contains("StartupWMClass=WebApp")
                        || line.contains("StartupWMClass=Chromium")
                        || line.contains("StartupWMClass=ICE-SSB")
                    {
                        is_webapp = true;
                    };

                    if line.contains("Name=") {
                        name = line.replace("Name=", "");
                    };

                    if line.contains("Icon=") {
                        icon = line.replace("Icon=", "");
                    };

                    if line.contains("Exec=") {
                        exec = line.replace("Exec=", "");
                    };

                    if line.contains("Categories=") {
                        category = line
                            .replace("Categories=", "")
                            .replace("GTK;", "")
                            .replace(';', "");
                    };

                    if line.contains("X-WebApp-Browser=") {
                        browser_name = line.replace("X-WebApp-Browser=", "");
                    };

                    if line.contains("X-WebApp-URL=") {
                        url = line.replace("X-WebApp-URL=", "");
                    };

                    if line.contains("X-WebApp-CustomParameters=") {
                        custom_parameters = line.replace("X-WebApp-CustomParameters=", "");
                    };

                    if line.contains("X-WebApp-Isolated=") {
                        isolate_profile = line.replace("X-WebApp-Isolated=", "") == "true"
                    };

                    if line.contains("X-WebApp-Navbar=") {
                        navbar = line.replace("X-WebApp-Navbar=", "") == "true"
                    };

                    if line.contains("X-WebApp-PrivateWindow=") {
                        is_incognito = line.replace("X-WebApp-PrivateWindow=", "") == "true"
                    };
                }
                Err(e) => eprintln!("Error reading line: {}", e),
            }
        }

        if is_webapp && !name.is_empty() && !icon.is_empty() {
            is_valid = true
        }

        let web_browser = Browser::web_browser(browser_name);

        match web_browser {
            Some(web_browser) => {
                exec.split(' ').enumerate().for_each(|(n, arg)| {
                    if n > 0 && !arg.is_empty() {
                        args.push(arg.to_string())
                    }
                });

                Ok(WebAppLauncher {
                    path,
                    codename,
                    web_browser,
                    name,
                    icon,
                    is_valid,
                    exec,
                    args,
                    category,
                    url,
                    custom_parameters,
                    isolate_profile,
                    navbar,
                    is_incognito,
                })
            }
            None => Err(anyhow!("Cannot read web app launcher.")),
        }
    }

    fn create_firefox_userjs(&self, path: PathBuf) -> bool {
        let content = include_bytes!("../data/runtime/firefox/profile/user.js");

        let mut file = File::create(&path)
            .unwrap_or_else(|_| panic!("failed to create user.js in {:?}", path));

        file.write_all(content).is_ok()
    }

    fn create_user_chrome_css(&self, path: PathBuf, create_navbar: bool) -> bool {
        let user_chrome_css =
            include_bytes!("../data/runtime/firefox/profile/chrome/userChrome.css");

        let mut file = File::create(&path)
            .unwrap_or_else(|_| panic!("cant create userChrome.css in {:?}", path));

        if create_navbar {
            file.write_all(b"").is_ok()
        } else {
            file.write_all(user_chrome_css).is_ok()
        }
    }

    fn exec_firefox(&self, fork: &str) -> String {
        let mut profile_dir = home_dir();
        if fork == "firefox" {
            profile_dir.push(".var/app/org.mozilla.firefox/data/ice/firefox");
        } else if fork == "librewolf" {
            profile_dir.push(".var/app/io.gitlab.librewolf-community/data/ice/librewolf");
        } else if fork == "waterfox" {
            profile_dir.push(".var/app/net.waterfox.waterfox/data/ice/waterfox");
        };

        let profile_path = profile_dir.join(&self.codename);
        let user_js_path = profile_path.join("user.js");
        let mut user_chrome_css = profile_path.join("chrome");

        create_dir_all(&profile_path)
            .unwrap_or_else(|_| panic!("cant create profile dir in {:?}", &profile_path));
        create_dir_all(&user_chrome_css)
            .unwrap_or_else(|_| panic!("cant create chrome dir in {:?}", &user_chrome_css));

        user_chrome_css = user_chrome_css.join("userChrome.css");

        self.create_firefox_userjs(user_js_path);
        self.create_user_chrome_css(user_chrome_css, self.navbar);

        let profile_path = profile_path.to_str().unwrap();

        let mut exec_string = format!(
            "{} --class WebApp-{} --name WebApp-{} --profile {} --no-remote ",
            self.exec, self.codename, self.codename, profile_path
        );

        if self.is_incognito {
            exec_string.push_str("--private-window ");
        }

        if !self.custom_parameters.is_empty() {
            exec_string.push_str(&format!("{} ", self.custom_parameters));
        }

        exec_string.push_str(&self.url);

        exec_string
    }

    fn exec_chromium(&self) -> String {
        let mut exec_string = format!(
            "{} --app={} --class=WebApp-{} --name=WebApp-{} ",
            self.exec, self.url, self.codename, self.codename
        );

        if self.isolate_profile {
            let mut profile_dir = PathBuf::new();

            let mut xdg_data_home = home_dir();
            xdg_data_home.push(".local/share");
            let ice_dir = xdg_data_home.join("ice");
            profile_dir.push(ice_dir.join("profiles").join(&self.codename));

            let profile_path = profile_dir.to_str().unwrap();
            exec_string.push_str(&format!("--user-data-dir={} ", profile_path));
        }

        if self.is_incognito {
            if self.web_browser.name.starts_with("Microsoft Edge") {
                exec_string.push_str("--inprivate ");
            } else {
                exec_string.push_str("--incognito ");
            }
        }

        if !self.custom_parameters.is_empty() {
            exec_string.push_str(&format!("{} ", self.custom_parameters));
        }

        exec_string
    }

    fn exec_falkon(&self) -> String {
        let mut exec_string = String::new();

        if self.isolate_profile {
            let mut profile_dir = PathBuf::new();

            let mut xdg_data_home = home_dir();
            xdg_data_home.push(".local/share");
            let ice_dir = xdg_data_home.join("ice");
            profile_dir.push(ice_dir.join("profiles").join(&self.codename));

            let profile_path = profile_dir.to_str().unwrap();

            exec_string = format!(
                "{} --portable --wmclass WebApp-{} --profile {} ",
                self.exec, self.codename, profile_path
            );
        }

        if self.is_incognito {
            exec_string.push_str("--private-browsing ");
        }

        if !self.custom_parameters.is_empty() {
            exec_string.push_str(&format!("{} ", self.custom_parameters));
        }

        exec_string.push_str(&format!("--no-remote --current-tab {}", self.url));

        exec_string
    }

    fn exec_string(&self) -> String {
        match self.web_browser._type {
            BrowserType::Firefox => self.exec_firefox("firefox"),
            BrowserType::FirefoxFlatpak => self.exec_firefox("firefox"),
            BrowserType::Librewolf => self.exec_firefox("librewolf"),
            BrowserType::WaterfoxFlatpak => self.exec_firefox("waterfox"),
            BrowserType::Chromium => self.exec_chromium(),
            BrowserType::Falkon => self.exec_falkon(),
            _ => String::new(),
        }
    }

    pub fn create(&self) -> Result<()> {
        let mut output = File::create(&self.path)?;

        writeln!(output, "[Desktop Entry]")?;
        writeln!(output, "Version=1.0")?;
        writeln!(output, "Name={}", self.name)?;
        writeln!(output, "Comment=Web App")?;
        writeln!(output, "Exec={}", self.exec_string())?;
        writeln!(output, "Terminal=false")?;
        writeln!(output, "Type=Application")?;
        writeln!(output, "Icon={}", self.icon)?;
        writeln!(output, "Categories=GTK;{};", self.category)?;
        writeln!(output, "MimeType=text/html;text/xml;application/xhtml_xml;")?;
        writeln!(output, "StartupWMClass=WebApp-{}", self.codename)?;
        writeln!(output, "StartupNotify=true")?;
        writeln!(output, "X-MultipleArgs=false")?;
        writeln!(output, "X-WebApp-Browser={}", self.web_browser.name)?;
        writeln!(output, "X-WebApp-URL={}", self.url)?;
        writeln!(output, "X-WebApp-Navbar={}", self.navbar)?;
        writeln!(output, "X-WebApp-PrivateWindow={}", self.is_incognito)?;
        writeln!(output, "X-WebApp-Isolated={}", self.isolate_profile)?;
        writeln!(
            output,
            "X-WebApp-CustomParameters={}",
            self.custom_parameters
        )?;

        Ok(())
    }

    pub fn delete(&self) -> Result<()> {
        let exist = self.path.as_path().exists();

        match exist {
            true => {
                remove_file(&self.path)?;
            }
            false => {
                tracing::error!("file not found");
            }
        }

        let mut profile_dir = home_dir();

        match self.web_browser._type {
            BrowserType::FirefoxFlatpak => {
                profile_dir.push(".var/app/org.mozilla.firefox/data/ice/firefox")
            }
            BrowserType::Librewolf => {
                profile_dir.push(".var/app/io.gitlab.librewolf-community/data/ice/librewolf")
            }
            BrowserType::WaterfoxFlatpak => {
                profile_dir.push(".var/app/net.waterfox.waterfox/data/ice/waterfox")
            }
            _ => {}
        };

        let profile_path = profile_dir.join(&self.codename);

        if remove_dir_all(profile_path).is_ok() {
            tracing::info!("Removed firefox profile directory.");
        };

        Ok(())
    }
}

pub fn get_webapps() -> Vec<Result<WebAppLauncher>> {
    let mut webapps = Vec::new();

    match fs::read_dir(desktop_filepath("")) {
        Ok(entries) => {
            for entry in entries {
                match entry {
                    Ok(entry) => {
                        let entry_fn = entry.file_name();
                        let filename = entry_fn.to_str().unwrap();

                        if filename.starts_with("webapp-") && filename.ends_with(".desktop") {
                            let codename = filename.replace("webapp-", "").replace(".desktop", "");

                            let launcher = WebAppLauncher::read(entry.path(), codename);
                            webapps.push(launcher);
                        }
                    }
                    Err(e) => tracing::error!("Error reading directory: {}", e),
                }
            }
        }
        Err(_) => {
            create_dir_all(desktop_filepath("")).expect("Cannot create local applications dir");
        }
    }

    webapps
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserType {
    NoBrowser,
    Firefox,
    FirefoxFlatpak,
    Librewolf,
    WaterfoxFlatpak,
    Chromium,
    Falkon,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Browser {
    pub _type: BrowserType,
    pub name: String,
    pub exec: String,
    test: PathBuf,
}

impl AsRef<str> for Browser {
    fn as_ref(&self) -> &str {
        &self.name
    }
}

impl Browser {
    pub fn new(_type: BrowserType, name: &str, exec: &str, test_path: &str) -> Self {
        let name = name.to_string();

        let mut test = PathBuf::new();
        let mut exe_path = PathBuf::new();

        let base = home_dir();
        let data_home = base.join(".local/share");

        if exec.starts_with(".local/share/") {
            let flatpak_path: Vec<&str> = exec.split(".local/share/").collect();
            let path = data_home.join(flatpak_path[1]);
            exe_path.push(path);
        } else {
            exe_path.push(exec)
        }

        if test_path.starts_with(".local/share") {
            let flatpak_path: Vec<&str> = test_path.split(".local/share/").collect();
            let path = data_home.join(flatpak_path[1]);
            test.push(path);
        } else {
            test.push(test_path)
        }

        let exec = exe_path.to_str().unwrap().to_string();

        Self {
            _type,
            name,
            exec,
            test,
        }
    }

    pub fn web_browser(name: String) -> Option<Browser> {
        let supported = get_supported_browsers();
        supported.into_iter().find(|b| b.name == name)
    }

    pub fn is_installed(&self) -> bool {
        !matches!(self._type, BrowserType::NoBrowser)
    }
}

pub fn get_supported_browsers() -> Vec<Browser> {
    let mut test_browsers: Vec<Browser> = Vec::new();

    let native_browsers = native_browsers();
    let flatpak_browsers = flatpak_browsers();

    test_browsers.extend(native_browsers);
    test_browsers.extend(flatpak_browsers);

    let mut browsers = Vec::new();

    for browser in test_browsers {
        let exists = browser.test.as_path().try_exists();

        match exists {
            Ok(found) => match found {
                true => browsers.push(browser),
                false => continue,
            },
            Err(_) => continue,
        }
    }
    browsers.insert(
        0,
        Browser::new(BrowserType::NoBrowser, "Select browser", "", ""),
    );

    browsers
}

pub fn get_icon_name_from_url(url: &str) -> String {
    match Url::parse(url) {
        Ok(url) => match url.host_str() {
            Some(host) => {
                let parts: Vec<&str> = host.split('.').collect();
                parts[parts.len() - 2].to_string()
            }
            None => String::new(),
        },
        Err(_) => String::new(),
    }
}

pub async fn find_icon(path: PathBuf, icon_name: &str) -> Vec<String> {
    let mut icons: Vec<String> = Vec::new();

    for entry in WalkDir::new(path).into_iter().filter_map(|e| e.ok()) {
        if let Some(filename) = entry.file_name().to_str() {
            if filename.contains(icon_name) {
                if let Some(path) = entry.path().to_str() {
                    if let Ok(buffer) = tokio::fs::read_to_string(&mut path.to_string()).await {
                        let options = usvg::Options::default();
                        if let Ok(parsed) =
                            usvg::Tree::from_str(&buffer, &options, &fontdb::Database::new())
                        {
                            let size = parsed.size();
                            if size.width() >= 64.0 && size.height() >= 64.0 {
                                if !icons.contains(&path.to_string()) {
                                    icons.push(path.to_string())
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    icons
}

pub async fn find_icons(icon_name: String, url: String) -> Vec<String> {
    let mut result: Vec<String> = Vec::new();

    result.extend(find_icon(icons_location(), &icon_name).await);

    if url_valid(&url) {
        if let Ok(data) = download_favicon(&url).await {
            result.extend(data)
        }
    };

    result
}

pub async fn search_user_icons() -> Vec<String> {
    let mut result: Vec<String> = Vec::new();
    let user_folder = icons_location().join("MyIcons");

    if let Ok(mut entries) = tokio::fs::read_dir(user_folder).await {
        while let Some(entry) = entries.next_entry().await.unwrap() {
            // Iterate over each entry asynchronously
            let path = entry.path();
            if path.is_file() {
                result.push(path.to_str().unwrap().to_string());
            }
        }
    }
    result
}
pub async fn download_favicon(url: &str) -> Result<Vec<String>> {
    let mut favicons = Vec::new();

    let content = Client::new()
        .get(url)
        .send()
        .await
        .expect("sending request")
        .text()
        .await
        .expect("getting content");

    let document = Html::parse_document(&content);
    let head = Selector::parse("head").unwrap();
    let link = Selector::parse("link").unwrap();
    let meta = Selector::parse("meta").unwrap();

    for head in document.select(&head) {
        let fragment = Html::parse_document(&head.html());

        for link in fragment.select(&link) {
            if link.attr("rel") == Some("icon") {
                let val = link.value().attr("href").unwrap();

                favicons.push(val.to_string());
            }
        }

        for meta in fragment.select(&meta) {
            if meta.value().attr("property") == Some("og:image") {
                let val = meta.value().attr("content").unwrap();

                favicons.push(val.to_string());
            }
        }
    }

    Ok(favicons)
}

pub fn move_icon(path: String, output_name: String) -> String {
    let user_icons = icons_location().join("MyIcons");
    create_dir_all(user_icons.clone()).expect("cant create icons folder");

    let extension = if is_svg(&path) {
        String::from("svg")
    } else {
        PathBuf::from_str(&path)
            .unwrap()
            .extension()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string()
    };

    let save_path = user_icons
        .join(format!("{}.{}", output_name.replace(' ', ""), extension))
        .to_str()
        .unwrap()
        .to_string();

    if url_valid(&path) {
        let response = reqwest::blocking::get(&path).expect("sending request");

        if response.status().is_success() {
            let content = response.bytes().expect("getting image bytes");

            let mut file = File::create(&save_path).expect("creating file");
            file.write_all(&content).expect("saving image");
        }
    } else if !path.contains(&save_path) {
        copy(&path, &save_path).expect("saving image");
    }

    save_path
}

pub async fn image_handle(path: String) -> Option<iconpicker::Icon> {
    let mut data: Vec<_> = Vec::new();
    let pathbuf = PathBuf::from_str(&path).unwrap();

    if url_valid(&path) {
        data.extend(
            Client::new()
                .get(&path)
                .send()
                .await
                .unwrap()
                .bytes()
                .await
                .unwrap()
                .to_vec(),
        );
    } else if let Ok(mut file) = File::open(&pathbuf) {
        let mut buffer = Vec::new();

        if pathbuf.is_file() {
            file.read_to_end(&mut buffer).unwrap();
        };

        data.extend(buffer);
    };

    if is_svg(&path) {
        let handle = widget::svg::Handle::from_memory(data);

        return Some(iconpicker::Icon::new(
            iconpicker::IconType::Svg(handle),
            path,
        ));
    } else if let Ok(image) = ImageReader::new(Cursor::new(&data))
        .with_guessed_format()
        .unwrap()
        .decode()
    {
        if image.width() >= 96 && image.height() >= 96 {
            let handle = widget::image::Handle::from_memory(data);

            return Some(iconpicker::Icon::new(
                iconpicker::IconType::Raster(handle),
                path,
            ));
        }
    };

    None
}
