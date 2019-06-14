use crate::app::{self, column_view::ColumnView, row_view::RowView};
use data_table::DataColumn;
use diwata_intel::{DataRow, Field, Tab};
use sauron::{
    html::{attributes::*, events::*, units::*, *},
    Component, Node,
};

use crate::app::{column_view, row_view};
use diwata_intel::data_container::Page;

#[derive(Debug, Clone)]
pub enum Msg {
    ColumnMsg(usize, column_view::Msg),
    RowMsg(usize, row_view::Msg),
}

pub struct PageView {
    pub data_columns: Vec<DataColumn>,
    pub row_views: Vec<RowView>,
    /// Which columns of the rows are to be frozen on the left side of the table
    frozen_rows: Vec<usize>,
    frozen_columns: Vec<usize>,
    pub scroll_top: i32,
    scroll_left: i32,
    allocated_width: i32,
    allocated_height: i32,
    /// the total number of rows count in the table
    total_rows: usize,
    current_page: usize,
}

impl PageView {
    pub fn new(data_columns: &Vec<DataColumn>, page: &Page) -> Self {
        let mut page_view = PageView {
            data_columns: data_columns.clone(),
            row_views: vec![],
            frozen_rows: vec![],
            frozen_columns: vec![],
            scroll_top: 0,
            scroll_left: 0,
            allocated_width: 0,
            allocated_height: 0,
            total_rows: 0,
            current_page: 1,
        };
        page_view.set_page(page, 1, 1);
        page_view
    }

    pub fn get_row(&self, row_index: usize) -> Option<&RowView> {
        self.row_views.iter().find(|row| row.index == row_index)
    }

    fn fields_to_data_columns(fields: &[Field]) -> Vec<DataColumn> {
        fields.iter().map(Self::field_to_data_column).collect()
    }

    fn field_to_data_column(field: &Field) -> DataColumn {
        DataColumn {
            name: field.name.clone(),
            description: field.description.clone(),
            tags: vec![],
            data_type: field.get_data_type().clone(),
            is_primary: field.is_primary,
        }
    }

    pub fn set_page(&mut self, page: &Page, current_page: usize, total_rows: usize) {
        sauron::log!("setting pages in page_view: {:#?}", page);
        self.set_data_rows(&page.rows, current_page, total_rows);
    }

    /// replace all the data with a new data row
    /// TODO: also update the freeze_columns for each row_views
    pub fn set_data_rows(
        &mut self,
        data_row: &Vec<DataRow>,
        current_page: usize,
        total_rows: usize,
    ) {
        self.row_views = data_row
            .into_iter()
            .enumerate()
            .map(|(index, row)| RowView::new(index, row, &self.data_columns))
            .collect();
        self.update_freeze_columns();
        self.total_rows = total_rows;
        self.current_page = current_page;
    }

    pub fn freeze_rows(&mut self, rows: &Vec<usize>) {
        self.frozen_rows = rows.clone();
        self.update_frozen_rows();
    }

    /// call this is frozen rows selection are changed
    fn update_frozen_rows(&mut self) {
        let frozen_rows = &self.frozen_rows;
        self.row_views
            .iter_mut()
            .enumerate()
            .for_each(|(index, row_view)| {
                if frozen_rows.contains(&index) {
                    row_view.set_is_frozen(true)
                } else {
                    row_view.set_is_frozen(false)
                }
            })
    }

    fn frozen_row_height(&self) -> i32 {
        self.frozen_rows.len() as i32 * RowView::row_height() //use the actual row height
    }

    fn frozen_column_width(&self) -> i32 {
        self.frozen_columns.len() as i32 * 200 //use the actual column sizes for each frozen columns
    }
    /// Keep updating which columns are frozen
    /// call these when new rows are set or added
    pub fn update_freeze_columns(&mut self) {
        let frozen_columns = self.frozen_columns.clone();
        self.row_views
            .iter_mut()
            .for_each(|row_view| row_view.freeze_columns(frozen_columns.clone()))
    }

    pub fn freeze_columns(&mut self, columns: &Vec<usize>) {
        self.frozen_columns = columns.clone();
        self.update_freeze_columns();
    }

    /// This is the allocated height set by the parent tab
    pub fn set_allocated_size(&mut self, (width, height): (i32, i32)) {
        self.allocated_width = width;
        self.allocated_height = height;
    }

    /// TODO: include the height of the frozen rows
    pub fn calculate_normal_rows_size(&self) -> (i32, i32) {
        let height = self.allocated_height
            - self.frozen_row_height()
            - self.calculate_needed_height_for_auxilliary_spaces();
        let width = self.allocated_width
            - self.frozen_column_width()
            - self.calculate_needed_width_for_auxilliary_spaces();
        let clamped_height = if height < 0 { 0 } else { height };
        let clamped_width = if width < 0 { 0 } else { width };
        (clamped_width, clamped_height)
    }

    fn calculate_normal_rows_height(&self) -> i32 {
        self.calculate_normal_rows_size().1
    }

    fn calculate_normal_rows_width(&self) -> i32 {
        self.calculate_normal_rows_size().0
    }

    /// height from the columns names, padding, margins and borders
    pub fn calculate_needed_height_for_auxilliary_spaces(&self) -> i32 {
        120
    }

    pub fn calculate_needed_width_for_auxilliary_spaces(&self) -> i32 {
        80
    }

    /// calculate the height of the content
    /// it rows * row_height
    pub fn height(&self) -> i32 {
        sauron::log!("row views: {}", self.row_views.len());
        self.row_views.len() as i32 * RowView::row_height()
    }

    /// These are values in a row that is under the frozen columns
    /// Can move up and down
    fn view_frozen_columns(&self) -> Node<Msg> {
        // can move up and down
        ol(
            [
                class("frozen_columns"),
                styles([("margin-top", px(-self.scroll_top))]),
            ],
            self.row_views
                .iter()
                .enumerate()
                .filter(|(index, _row_view)| !self.frozen_rows.contains(index))
                .map(|(index, row_view)| {
                    // The checkbox selection and the rows of the frozen
                    // columns
                    div(
                        [class("selector_and_frozen_column_row")],
                        [
                            input([r#type("checkbox")], []),
                            row_view
                                .view_frozen_columns()
                                .map(move |row_msg| Msg::RowMsg(index, row_msg)),
                        ],
                    )
                })
                .collect::<Vec<Node<Msg>>>(),
        )
    }

    /// The rest of the columns and move in any direction
    fn view_normal_rows(&self) -> Node<Msg> {
        // can move: left, right, up, down
        ol(
            [],
            self.row_views
                .iter()
                .enumerate()
                .map(|(index, row_view)| {
                    row_view
                        .view()
                        .map(move |row_msg| Msg::RowMsg(index, row_msg))
                })
                .collect::<Vec<Node<Msg>>>(),
        )
    }

    pub fn update(&mut self, msg: Msg) -> app::Cmd {
        match msg {
            Msg::RowMsg(row_index, row_msg) => app::Cmd::none(),
            Msg::ColumnMsg(column_index, column_msg) => app::Cmd::none(),
        }
    }

    /// A grid of 2x2  containing 4 major parts of the table
    pub fn view(&self) -> Node<Msg> {
        div(
            [class(format!("total_rows: {}", self.row_views.len()))],
            [text(self.row_views.len()), self.view_normal_rows()],
        )
    }
}