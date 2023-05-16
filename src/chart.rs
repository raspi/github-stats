use std::path::PathBuf;
use std::collections::HashMap;
use chrono::{Days, NaiveDate, Utc};
use std::error::Error;
use plotters::backend::SVGBackend;
use plotters::prelude::{BLUE, Color, IntoFont, Palette, Palette99, PointSeries, WHITE};
use plotters::chart::{ChartBuilder, SeriesLabelPosition};
use human_format::Formatter;
use plotters::element::{Circle, EmptyElement, Rectangle, Text};
use plotters::drawing::IntoDrawingArea;

pub struct ChartGenerator {
    data: HashMap<
        NaiveDate, HashMap<u8, u64>
    >,
    renames: HashMap<u8, String>,
    // For average(s)
    counts: HashMap<u8, u64>,
    width: u32,
    height: u32,
    filename: PathBuf,
    title: String,
    // How many days, usually 30
    days: u32,
}

impl ChartGenerator {
    pub fn new(
        title: String,
        filename: PathBuf,
        renames: HashMap<u8, String>,
        days: u32,
    ) -> Self {
        Self {
            title: title,
            data: Default::default(),
            renames: renames,
            counts: Default::default(),
            width: 640,
            height: 480,
            filename: filename,
            days: days,
        }
    }

    // Add chart data points
    pub fn add(
        &mut self,
        d: NaiveDate,
        data: HashMap<u8, u64>,
    ) {
        for (k, v) in &data {
            *self.counts
                .entry(*k)
                .or_insert(0) += v;
        }

        // insert data
        self.data.insert(d, data);
    }

    // Render SVG
    pub fn render(&mut self) -> Result<(), Box<dyn Error>> {
        let mut max_y: u64 = 0;

        for (_, vals) in self.data.clone() {
            for (_, val) in vals {
                if val > max_y {
                    max_y = val;
                }
            }
        }

        if max_y < 10 {
            // Minimum 10, so that the zeroes don't go over the title
            max_y = 10;
        }

        let root = SVGBackend::new(
            self.filename.as_path(),
            (self.width, self.height),
        ).into_drawing_area();

        root.fill(&WHITE)?;
        let root = root.margin(5, 5, 20, 30);

        let now_naive = Utc::now().date_naive();

        // construct chart context
        let mut chart = ChartBuilder::on(&root)
            // Set the caption of the chart
            .caption(
                &self.title,
                ("sans-serif", 30).into_font(),
            )
            // Set the size of the label region
            .x_label_area_size(35)// days
            .y_label_area_size(30)// counts
            .build_cartesian_2d(
                0u32..self.days, // days 0-29 / 1-30
                0u64..((max_y + 9) / 10 * 10), // count of views / clones rounded to nearest ten
            )?
            ;


        // draw a mesh
        chart
            .configure_mesh()
            .x_desc(
                format!(
                    "Dates {:?} - {:?}",
                    now_naive,
                    now_naive.clone().checked_sub_days(Days::new(self.days as u64)).expect("date error")
                )
            )
            .y_desc("Count")
            //.y_max_light_lines(1)
            // maximum number of labels allowed for each axis
            .x_labels(15)// days
            .y_labels(10)// counts

            // format of the label text
            .y_label_formatter(
                &|y| {
                    // View / clone counts
                    if *y < 10000 {
                        y.to_string()
                    } else {
                        Formatter::new()
                            .with_decimals(1)
                            .format(*y as f64)
                    }
                }
            )
            .x_label_formatter(
                &|x| {
                    // Date
                    format!(
                        "{:?}",
                        now_naive.checked_sub_days(
                            Days::new((*x) as u64)
                        ).expect("??")
                    )
                }
            )
            .draw()?;


        for typeid in 0u8..2 {
            // Add empty if missing
            self.counts.entry(typeid).or_insert(0);

            let mut now = now_naive.clone().to_owned();
            let mut data: Vec<(u32, u64)> = vec![];

            // Last N days of data
            for day_index in 0..self.days {
                match self.data.get(&now) {
                    None => { data.push((day_index, 0)) }
                    Some(d) => {
                        let val = match d.get(&typeid) {
                            None => { 0 }
                            Some(v) => { *v }
                        };

                        data.push((day_index, val));
                    }
                };

                now = match now.checked_sub_days(Days::new(1)) {
                    None => { panic!("invalid date"); }
                    Some(d) => { d }
                };
            }

            let color = Palette99::pick(typeid as usize).mix(0.9);

            // draw points
            chart
                .draw_series(
                    PointSeries::of_element(
                        data,
                        5,
                        color.clone().to_rgba(),
                        &|c, s, st| {
                            return EmptyElement::at(c)
                                + Circle::new((0, 0), s, st.filled()) // At this point, the new pixel coordinate is established
                                + Text::new(format!("{}", c.1), (-5, -18), ("sans-serif", 15).into_font());
                        },
                    )
                )?
                .label(
                    // Add legend name
                    match self.renames.get(&typeid) {
                        None => { String::from("?") }
                        Some(n) => {
                            // Add total counts
                            format!("{} ({})", n, self.counts[&typeid])
                        }
                    }
                )
                .legend(move |(x, y)|
                    Rectangle::new(
                        [(x - 10, y - 5), (x, y)],
                        color.clone().to_rgba().filled(),
                    )
                );
        } // /for

        // Legend
        chart
            .configure_series_labels()
            .position(SeriesLabelPosition::UpperRight)
            .margin(20)
            .legend_area_size(0)
            .border_style(BLUE)
            .background_style(BLUE.mix(0.1))
            .label_font(("sans-serif", 20))
            .draw()?
        ;

        root.present()?;

        Ok(())
    }

    // Reset internal data
    pub fn reset(&mut self) {
        self.data = Default::default();
        self.counts = Default::default();
    }
}
