use std::path::PathBuf;

use image::{DynamicImage, ImageError, Rgba};
use indicatif::{ProgressBar, ProgressStyle};
use structopt::{
    clap::{_clap_count_exprs, arg_enum},
    StructOpt,
};

fn pixel_max(Rgba { data, .. }: &Rgba<u8>) -> u8 {
    data[..3].iter().max().cloned().unwrap_or_default()
}

fn pixel_min(Rgba { data, .. }: &Rgba<u8>) -> u8 {
    data[..3].iter().min().cloned().unwrap_or_default()
}

fn pixel_chroma(pixel: &Rgba<u8>) -> u8 {
    pixel_max(pixel) - pixel_min(pixel)
}

fn pixel_hue(pixel: &Rgba<u8>) -> u8 {
    let c = pixel_chroma(pixel);

    if c == 0 {
        return 0;
    }

    let Rgba { data, .. } = pixel;

    match data[..3].iter().enumerate().max_by_key(|&(_, e)| e) {
        Some((0, _)) => (data[1] as i16 - data[2] as i16).abs() as u8 / c * 43,
        Some((1, _)) => (data[2] as i16 - data[0] as i16).abs() as u8 / c * 43 + 85,
        Some((2, _)) => (data[0] as i16 - data[1] as i16).abs() as u8 / c * 43 + 171,
        _ => 0,
    }
}

arg_enum! {
    enum SortHeuristic {
        Luma,
        Brightness,
        Max,
        Min,
        Chroma,
        Hue,
        Saturation,
        Value,
        Red,
        Blue,
        Green,
    }
}

impl SortHeuristic {
    fn func(&self) -> Box<Fn(&Rgba<u8>) -> u8> {
        match self {
            SortHeuristic::Red => Box::new(|Rgba { data, .. }| data[0]),
            SortHeuristic::Green => Box::new(|Rgba { data, .. }| data[1]),
            SortHeuristic::Blue => Box::new(|Rgba { data, .. }| data[2]),
            SortHeuristic::Max => Box::new(pixel_max),
            SortHeuristic::Min => Box::new(pixel_min),
            SortHeuristic::Chroma => Box::new(pixel_chroma),
            SortHeuristic::Hue => Box::new(pixel_hue),
            SortHeuristic::Saturation => Box::new(|p| match pixel_max(p) {
                0 => 0,
                v => pixel_chroma(p) / v,
            }),
            SortHeuristic::Value => Box::new(pixel_max),
            SortHeuristic::Brightness => Box::new(|Rgba { data, .. }| {
                data[0] / 3
                    + data[1] / 3
                    + data[2] / 3
                    + (data[0] % 3 + data[1] % 3 + data[2] % 3) / 3
            }),
            SortHeuristic::Luma => Box::new(|Rgba { data, .. }| {
                // https://stackoverflow.com/a/596241
                ((data[0] as u16 * 2 + data[1] as u16 + data[2] as u16 * 4) >> 3) as u8
            }),
        }
    }
}

#[derive(StructOpt)]
#[structopt(about = "Sort the pixels in an image")]
#[structopt(raw(setting = "structopt::clap::AppSettings::ColoredHelp"))]
#[structopt(rename_all = "kebab-case")]
struct Cli {
    /// Input file
    #[structopt(parse(try_from_str))]
    file: PathBuf,
    /// Output file
    #[structopt(short, parse(try_from_str))]
    output: Option<PathBuf>,
    /// Minimum value to sort
    #[structopt(short, default_value = "0")]
    minimum: u8,
    /// Maximum value to sort
    #[structopt(short = "x", default_value = "255")]
    maximum: u8,
    /// Sort heuristic to use
    #[structopt(
        short,
        default_value = "luma",
        raw(
            possible_values = "&SortHeuristic::variants()",
            case_insensitive = "true",
            set = "structopt::clap::ArgSettings::NextLineHelp"
        )
    )]
    function: SortHeuristic,
    /// Reverse the sort direction
    #[structopt(short)]
    reverse: bool,
    /// Sort outside specified range rather than inside
    #[structopt(short)]
    invert: bool,
    /// Sort vertically instead of horizontally
    #[structopt(short)]
    vertical: bool,
    /// Don't sort pixels that have zero alpha
    #[structopt(long)]
    mask_alpha: bool,
}

fn main() -> Result<(), ImageError> {
    let cli = Cli::from_args();

    eprintln!("Opening image at {:?}", cli.file);
    let mut img = image::open(&cli.file)?;

    if cli.vertical {
        img = img.rotate90();
    }

    let mut rgba = img.to_rgba();
    let (w, h) = rgba.dimensions();

    let prog = ProgressBar::new(h as u64);
    prog.set_draw_delta(h as u64 / 50);
    prog.set_prefix(&format!(
        "Sorting {}:",
        if cli.vertical { "columns" } else { "rows" }
    ));
    prog.set_style(ProgressStyle::default_bar().template("{prefix} {wide_bar} {pos:>4}/{len}"));
    prog.tick();

    for (idx_y, row) in rgba
        .clone()
        .pixels_mut()
        .collect::<Vec<_>>()
        .chunks_mut(w as usize)
        .enumerate()
    {
        let sort_fn = cli.function.func();
        let mask_fn = |p: &Rgba<u8>| !(cli.mask_alpha && p.data[3] == 0);

        let mut ctr = 0;
        while ctr < w as usize {
            // find the end of the current "good" sequence
            let numel = row[ctr..]
                .iter()
                .take_while(|p| {
                    let l = sort_fn(p);
                    (l >= cli.minimum && l <= cli.maximum) != cli.invert && mask_fn(p)
                })
                .count();

            // sort
            row[ctr..ctr + numel].sort_unstable_by(|l, r| {
                if cli.reverse {
                    sort_fn(r).cmp(&sort_fn(l))
                } else {
                    sort_fn(l).cmp(&sort_fn(r))
                }
            });

            ctr += numel;

            // continue until another value in the right range appears
            ctr += row[ctr..]
                .iter()
                .take_while(|p| {
                    let l = sort_fn(p);
                    (l < cli.minimum || l > cli.maximum) != cli.invert || !mask_fn(p)
                })
                .count();
        }

        for (idx_x, px) in row.iter().enumerate() {
            rgba.put_pixel(idx_x as u32, idx_y as u32, **px);
        }

        prog.inc(1);
    }

    prog.finish_with_message("Done sorting!");

    let mut img_out = DynamicImage::ImageRgba8(rgba);

    if cli.vertical {
        img_out = img_out.rotate270();
    }

    let file_out = if let Some(p) = cli.output {
        p
    } else {
        match (
            cli.file.parent(),
            cli.file.file_stem(),
            cli.file.extension(),
        ) {
            (None, _, _) | (_, None, _) | (_, _, None) => panic!("Invalid filename"),
            (Some(p), Some(b), Some(e)) => {
                let mut fname = b.to_owned();
                fname.push("_1.");
                fname.push(e);
                let mut pth = p.to_owned();
                pth.push(fname);
                pth
            }
        }
    };

    eprintln!("Saving file to {:?}", file_out);
    img_out.save(file_out)?;

    Ok(())
}
