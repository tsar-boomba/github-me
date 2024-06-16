use std::{
    collections::BTreeMap,
    fs,
    ops::AddAssign,
    sync::{atomic::AtomicBool, Arc, Mutex},
    time::Instant,
};

use gix::progress;
use lambda_runtime::{tracing, Error, LambdaEvent};
use octocrab::models;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use serde::Serialize;
use tokei::{Language, LanguageType};

const SEPARATOR: &str = "=================================";

#[derive(Debug, Serialize, Clone, Copy)]
struct SimpleLanguage {
    name: LanguageType,
    code: usize,
    blanks: usize,
    comments: usize,
}

impl SimpleLanguage {
    fn from_lang(ty: &LanguageType, lang: &Language) -> Self {
        Self {
            name: ty.clone(),
            code: lang.code,
            blanks: lang.blanks,
            comments: lang.comments,
        }
    }
}

impl AddAssign<&SimpleLanguage> for SimpleLanguage {
    fn add_assign(&mut self, rhs: &SimpleLanguage) {
        self.code += rhs.code;
        self.comments += rhs.comments;
        self.blanks += rhs.blanks;
    }
}

#[derive(Debug, Serialize)]
struct PerRepo {
    name: String,
    href: String,
    description: Option<String>,
    languages: Vec<SimpleLanguage>,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    // required to enable CloudWatch error logging by the runtime
    tracing::init_default_subscriber();
    dotenvy::dotenv().ok();

    octocrab::initialise(
        octocrab::Octocrab::builder()
            .personal_token(std::env::var("PERSONAL_ACCESS_TOKEN").unwrap())
            .build()
            .unwrap(),
    );

    // In prod, setup all the stuff for handling lambda
    #[cfg(not(debug_assertions))]
    {
        let func = lambda_runtime::service_fn(my_handler);
        lambda_runtime::run(func).await?;
    }

    // In dev, just run the stuff normally
    #[cfg(debug_assertions)]
    run().await?;

    Ok(())
}

async fn run() -> Result<(), Error> {
    let start_time = Instant::now();
    let octocrab = octocrab::instance();
    let exclude_repos_string = std::env::var("EXCLUDE_REPOS").unwrap_or_default();
    let exclude_repos = exclude_repos_string.split(",").collect::<Vec<_>>();

    let mut page = octocrab
        .current()
        .list_repos_for_authenticated_user()
        .affiliation("owner")
        .direction("desc")
        .sort("updated")
        .send()
        .await?;

    let mut repos =
        Vec::with_capacity(page.items.len() * page.number_of_pages().unwrap_or(1) as usize);

    loop {
        for repo in &page {
            if !repo.fork.is_some_and(|f| f) {
                repos.push(repo.clone());
            }
        }

        page = match octocrab.get_page::<models::Repository>(&page.next).await? {
            Some(next_page) => next_page,
            None => break,
        }
    }

    fs::remove_dir_all("/tmp/repo").ok();
    fs::create_dir("/tmp/repo").unwrap();

    let config = tokei::Config {
        types: Some(vec![
            LanguageType::Rust,
            LanguageType::C,
            LanguageType::Cpp,
            LanguageType::JavaScript,
            LanguageType::TypeScript,
            LanguageType::Css,
            LanguageType::Html,
            LanguageType::Python,
            LanguageType::Java,
            LanguageType::Sh,
            LanguageType::Tsx,
            LanguageType::Jsx,
            LanguageType::Toml,
            LanguageType::Markdown,
            LanguageType::Svelte,
            LanguageType::Vue,
            LanguageType::Sass,
            LanguageType::CMake,
            LanguageType::CppHeader,
            LanguageType::Zig,
            LanguageType::Go,
            LanguageType::Dockerfile,
            LanguageType::Yaml,
            LanguageType::Json,
        ]),
        ..Default::default()
    };
    let total = Arc::new(Mutex::new(Vec::<SimpleLanguage>::with_capacity(
        config.types.as_ref().unwrap().len(),
    )));
    let per_repo_stats = Arc::new(Mutex::new(Vec::<PerRepo>::with_capacity(repos.len())));

    // Process largest repos first
    repos.sort_unstable_by(|a, b| {
        b.size
            .clone()
            .unwrap_or_default()
            .cmp(&a.size.clone().unwrap_or_default())
    });

    // Rayon is actually amazing. Really shows the strengths of Rust
    repos.into_par_iter().for_each({
        let total = total.clone();
        let per_repo_stats = per_repo_stats.clone();
        move |repo| {
            let clone_start = Instant::now();
            let repo_path = format!("/tmp/repo/{}", repo.name);
            println!(
                "Cloning: \"{}\"; Size: {}",
                repo.name,
                repo.size
                    .map(|n| human_bytes::human_bytes(n * 1000))
                    .unwrap_or_default()
            );
            let mut url = repo.clone_url.unwrap();
            url.set_username("tsar-boomba").unwrap();
            url.set_password(Some(&std::env::var("PERSONAL_ACCESS_TOKEN").unwrap()))
                .unwrap();

            let gix_url = gix::Url::from_bytes(url.as_str().try_into().unwrap()).unwrap();

            let (mut checkout, _) = gix::prepare_clone(gix_url, &repo_path)
                .unwrap()
                .with_shallow(gix::remote::fetch::Shallow::DepthAtRemote(
                    1.try_into().unwrap(),
                ))
                .fetch_then_checkout(progress::Discard, &AtomicBool::new(false))
                .unwrap();

            checkout
                .main_worktree(progress::Discard, &AtomicBool::new(false))
                .unwrap();

            println!(
                "Done cloning \"{}\" in {:.2} seconds!",
                repo.name,
                (Instant::now() - clone_start).as_secs_f64()
            );

            // tokei stuff
            let start_analyzing = Instant::now();
            let mut languages = tokei::Languages::new();
            println!("Analyzing \"{}\"...", repo.name);
            languages.get_statistics(
                &[&repo_path],
                &["build", "package-lock.json", "pnpm-lock.yaml"],
                &config,
            );
            println!(
                "Done analyzing \"{}\" in {:.2} seconds!",
                repo.name,
                (Instant::now() - start_analyzing).as_secs_f64()
            );

            for (ty, lang) in &languages {
                let mut total_lock = total.lock().unwrap();
                if let Some(total_lang) = total_lock.iter_mut().find(|lang| &lang.name == ty) {
                    *total_lang += &SimpleLanguage::from_lang(ty, lang);
                } else {
                    total_lock.push(SimpleLanguage::from_lang(ty, &lang));
                }
            }

            if !exclude_repos.contains(&repo.name.as_str()) && !repo.private.is_some_and(|p| p) {
                // Only include in per-repo if the repo is public and not excluded
                per_repo_stats.lock().unwrap().push(PerRepo {
                    languages: languages
                        .iter()
                        .map(|(lang, stat)| SimpleLanguage::from_lang(lang, stat))
                        .collect(),
                    name: repo.name.clone(),
                    href: repo.html_url.unwrap().to_string(),
                    description: repo.description,
                });
            } else {
                println!("Excluding \"{}\" from per-repo stats.", repo.name);
            }

            fs::remove_dir_all(&repo_path).unwrap();
            println!(
                "Done with \"{}\" in {:.2} seconds!",
                repo.name,
                (Instant::now() - clone_start).as_secs_f64()
            );
        }
    });

    println!(
        "{SEPARATOR}\n\nFinished all in {:.2} seconds!!!",
        (Instant::now() - start_time).as_secs_f64()
    );

    println!("Starting post-processing!");
    let post_start = Instant::now();
    let mut total = Arc::try_unwrap(total).unwrap().into_inner().unwrap();
    let mut per_repo_stats = Arc::try_unwrap(per_repo_stats)
        .unwrap()
        .into_inner()
        .unwrap();

    combine_ts_tsx(&mut total);

    // Manual adjustment for code done for contract work
    total
        .iter_mut()
        .find(|l| l.name == LanguageType::Rust)
        .unwrap()
        .code += 15673;

    total
        .iter_mut()
        .find(|l| l.name == LanguageType::TypeScript)
        .unwrap()
        .code += 4333;

    // Sort so that the repo with the most code is at the top
    per_repo_stats.sort_unstable_by(|a, b| total_code(&b.languages).cmp(&total_code(&a.languages)));

    // In each repo, sort languages by most used
    for repo in &mut per_repo_stats {
        combine_ts_tsx(&mut repo.languages);
        repo.languages.sort_unstable_by(|a, b| b.code.cmp(&a.code));
    }

    total.sort_unstable_by(|a, b| b.code.cmp(&a.code));

    println!(
        "Post-processing complete in {:.2} seconds",
        (Instant::now() - post_start).as_secs_f64()
    );

    common::save_stats(
        &serde_json::to_string(&total).unwrap(),
        &serde_json::to_string(&per_repo_stats).unwrap(),
    )
    .await?;

    println!(
        "All processing complete in {:.2} seconds",
        (Instant::now() - start_time).as_secs_f64()
    );

    Ok(())
}

fn total_code(languages: &[SimpleLanguage]) -> usize {
    let mut total = 0;

    for lang in languages {
        total += lang.code;
    }

    total
}

fn combine_ts_tsx(langs: &mut Vec<SimpleLanguage>) {
    let Some((tsx_idx, tsx)) = langs
        .iter()
        .enumerate()
        .find(|(_, l)| l.name == LanguageType::Tsx)
    else {
        return;
    };
    let tsx = tsx.clone();

    // Combine tsx and typescript into typescript
    let Some(ts) = langs
        .iter_mut()
        .find(|l| l.name == LanguageType::TypeScript)
    else {
        return;
    };

    *ts += &tsx;

    langs.swap_remove(tsx_idx);
}

pub(crate) async fn my_handler(_: LambdaEvent<serde_json::Value>) -> Result<(), Error> {
    run().await
}
