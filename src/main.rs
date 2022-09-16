use canvasapi::models::user::UserProfile;
use canvasapi::prelude::{Canvas, CanvasInformation, Submission};
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, FuzzySelect, Input, MultiSelect};
use dotenv::dotenv;
use futures::prelude::*;
use futures::stream::FuturesOrdered;
use once_cell::unsync::Lazy;
use regex::Regex;
use std::env;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use tokio::fs;

const README_DISCLAIMER: &str =
    "by submitting this file to carmen, i certify that i have performed all";

#[derive(Debug, strum::Display)]
enum Errors {
    AttachmentNotFound,
    InvalidSelection,
}

impl std::error::Error for Errors {}

#[derive(Debug)]
struct UserSubmission {
    user_profile: UserProfile,
    submission: Submission,
}

impl UserSubmission {
    async fn download_submission(self) -> Result<DownloadedSubmission, Box<dyn std::error::Error>> {
        let attachment = &self
            .submission
            .attachments
            .as_ref()
            .ok_or(Errors::AttachmentNotFound)?[0];

        let resp = reqwest::get(&attachment.url).await?;
        let body = std::io::Cursor::new(resp.bytes().await?);

        let path = PathBuf::from(&self.user_profile.sortable_name);
        let path_move = path.clone();
        tokio::task::spawn_blocking(move || zip_extract::extract(body, &path_move, true)).await??;

        Ok(DownloadedSubmission {
            user_profile: self.user_profile,
            path,
        })
    }
}

#[derive(Debug)]
struct DownloadedSubmission {
    user_profile: UserProfile,
    path: PathBuf,
}

impl DownloadedSubmission {
    async fn grade(&self) -> Result<(), Box<dyn std::error::Error>> {
        println!("Grading {}", self.user_profile.sortable_name.bright_blue());

        let mut entries = fs::read_dir(&self.path).await?;

        let mut files = vec![];

        while let Some(entry) = entries.next_entry().await? {
            if entry.file_type().await?.is_file() {
                files.push(File {
                    contents: fs::read_to_string(entry.path()).await.ok(),
                    name: entry.file_name().into_string().unwrap(),
                    path: entry.path(),
                });
            }
        }

        let lower_case_name = self.user_profile.sortable_name.to_lowercase();
        let last_name = lower_case_name.split(",").next().unwrap();

        let re: Lazy<Regex> =
            Lazy::new(|| Regex::new(r"(readme)|(\.c)|(\.h)|(makefile)|(.h)").unwrap());

        println!("File contains name:");

        files
            .iter()
            .filter(|f| re.is_match(&f.name.to_lowercase()))
            .for_each(|f| {
                let contains = match f
                    .contents
                    .clone()
                    .unwrap_or_default()
                    .to_lowercase()
                    .contains(&last_name)
                {
                    true => "✔".green(),
                    false => "✗".red(),
                };

                println!("\t{} {}", contains, f.name);
            });

        println!("File contains readme disclaimer:");

        files
            .iter()
            .filter(|f| f.name.to_lowercase().contains("readme"))
            .for_each(|f| {
                let contains = match f
                    .contents
                    .clone()
                    .unwrap_or_default()
                    .to_lowercase()
                    .contains(README_DISCLAIMER)
                {
                    true => "✔".green(),
                    false => "✗".red(),
                };

                println!("\t{} {}", contains, f.name);
            });

        press_enter_to_continue()?;

        files
            .iter()
            .filter(|f| re.is_match(&f.name.to_lowercase()))
            .map(File::open_file_in_editor)
            .collect::<Result<_, _>>()?;

        // use blocking command otherwise the inheritting of file descriptors
        // seems to deadlock the program
        Command::new("sh")
            .arg("-c")
            .arg(format!("cd '{}'; exec ${{SHELL:-sh}}", self.path.display()))
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .output()?;

        Ok(())
    }
}

#[derive(Debug)]
struct File {
    contents: Option<String>,
    path: PathBuf,
    name: String,
}

impl File {
    fn open_file_in_editor(&self) -> Result<(), Box<dyn std::error::Error>> {
        let editor = env::var("EDITOR").unwrap_or("vi".into());

        Command::new(editor)
            .arg(&self.path)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .output()?;

        Ok(())
    }
}

async fn fetch_user_profile(
    canvas: &CanvasInformation<'_>,
    user_id: usize,
) -> Result<UserProfile, Box<dyn std::error::Error>> {
    Ok(UserProfile::get_user_profile(user_id)?
        .fetch(canvas)
        .await?
        .inner())
}

fn press_enter_to_continue() -> Result<(), Box<dyn std::error::Error>> {
    println!("Press enter to continue");
    Command::new("sh")
        .arg("-c")
        .arg("read")
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .output()?;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();

    let base_url = env::var("CANVAS_BASE_URL").unwrap();
    let access_token = env::var("CANVAS_ACCESS_TOKEN").unwrap();
    let canvas = CanvasInformation::new(&base_url, &access_token);

    println!("Loading courses...");

    let courses = Canvas::get_courses()?.fetch(&canvas).await?.inner();

    let selection = FuzzySelect::with_theme(&ColorfulTheme::default())
        .with_prompt("Course")
        .items(
            &courses
                .iter()
                .filter_map(|c| c.name.clone())
                .collect::<Box<_>>(),
        )
        .interact()?;

    let course = &courses[selection];

    println!("Loading assignments...");

    let assignments: Vec<_> = course.get_assignments()?.fetch(&canvas).await?.inner();

    let selection = FuzzySelect::with_theme(&ColorfulTheme::default())
        .with_prompt("Assignment")
        .items(
            &assignments
                .iter()
                .filter_map(|a| a.name.clone())
                .collect::<Vec<_>>(),
        )
        .interact()?;

    let assignment = &assignments[selection];

    println!("Fetching available submissions...");

    let submissions = assignment.get_submissions()?.fetch(&canvas).await?.inner();

    let total_submissions = submissions.len();

    let division_count = Input::<usize>::with_theme(&ColorfulTheme::default())
        .with_prompt("Divison Count")
        .interact()?;

    let selection = FuzzySelect::with_theme(&ColorfulTheme::default())
        .with_prompt("Portion")
        .default(0)
        .items(
            &(1..=division_count)
                .map(|i| i.to_string())
                .collect::<Vec<_>>(),
        )
        .interact()?;

    let portion_length = total_submissions / division_count;
    let start = portion_length * selection;
    let end = if selection < division_count - 1 {
        start + portion_length
    } else {
        start + portion_length + (total_submissions % division_count)
    };

    let user_ids: Box<_> = submissions.iter().map(|s| s.user_id.unwrap()).collect();

    println!("Fetching selected portion...");

    let user_profiles = user_ids
        .into_iter()
        .map(|&id| fetch_user_profile(&canvas, id))
        .collect::<FuturesOrdered<_>>()
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;

    let mut user_submissions: Vec<_> = submissions
        .into_iter()
        .zip(user_profiles.into_iter())
        .map(|(submission, user_profile)| {
            Some(UserSubmission {
                submission,
                user_profile,
            })
        })
        .collect();

    user_submissions.sort_by(|a, b| {
        a.as_ref()
            .unwrap()
            .user_profile
            .sortable_name
            .cmp(&b.as_ref().unwrap().user_profile.sortable_name)
    });

    let mut user_submissions: Vec<_> = user_submissions.drain(start..end).collect();

    let selections = MultiSelect::with_theme(&ColorfulTheme::default())
        .with_prompt("Users to grade")
        .items(
            &user_submissions
                .iter()
                .map(|d| &d.as_ref().unwrap().user_profile.sortable_name)
                .collect::<Box<_>>(),
        )
        .defaults(&vec![true; user_submissions.len()])
        .interact()?;

    for s in selections {
        let mut submission = None;
        std::mem::swap(&mut user_submissions[s], &mut submission);

        let d = submission
            .ok_or(Errors::InvalidSelection)?
            .download_submission()
            .await?;

        d.grade().await?;
    }

    Ok(())
}
