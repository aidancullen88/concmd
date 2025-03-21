use crate::actions;
use crate::conf_api::{Name, Page, Space};
use crate::Config;

use cursive::views::{Dialog, SelectView};
use cursive::{Cursive, CursiveExt};

pub fn display(pick_page_ui: &mut Cursive) {
    /*
     * Generic function to build the display lists from returned lists
     * As long as the api return type impls Name, we can build a display
     * list from it
     */
    fn build_list<I>(list: I) -> SelectView<<I as Iterator>::Item>
    where
        I: Iterator,
        // Name ensures that the items have a label representation
        I::Item: Name,
        I::Item: Sync,
        I::Item: Send,
        I::Item: 'static,
    {
        let items = list.map(|s| (s.get_name(), s));
        SelectView::new().with_all(items)
    }

    // Config data is loaded in main() to avoid lifetime issues with
    // the callback below
    let config = pick_page_ui
        .user_data::<Config>()
        .expect("Config should always be loaded")
        .clone();

    // API call to get the space list
    let spaces = crate::actions::load_space_list(&config).unwrap();

    let space_select = build_list(spaces.into_iter()).on_submit(on_space_select);

    fn on_space_select(s: &mut Cursive, space: &Space) {
        // Config data is loaded in main() to avoid lifetime issues with
        // the callback below
        let config = s
            .user_data::<Config>()
            .expect("Config should always be loaded to cursive");
        // API call to get the page list
        let page_list = crate::actions::load_page_list_for_space(config, &space.id).unwrap();
        let page_select = build_list(page_list.into_iter()).on_submit(on_page_select);
        s.pop_layer();
        s.add_layer(Dialog::around(page_select).title(format!("Pages in {}", &space.name)));
    }

    fn on_page_select(s: &mut Cursive, page: &Page) {
        s.set_user_data(page.id.clone());
        s.quit();
    }

    pick_page_ui.add_layer(Dialog::around(space_select).title("Spaces"));

    pick_page_ui.run();

    if let Some(id) = pick_page_ui.user_data::<String>() {
        actions::edit_page(&config, id);
    } else {
        std::process::exit(1)
    }
}
