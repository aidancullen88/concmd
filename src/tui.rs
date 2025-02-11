use crate::conf_api::Name;
use crate::conf_api::Space;
use crate::Config;

use cursive::views::{Dialog, SelectView};
use cursive::{Cursive, CursiveExt};

pub fn display(siv: &mut Cursive) {
    fn build_list<I>(list: I) -> SelectView<<I as Iterator>::Item>
    where
        I: Iterator,
        I::Item: Name,
        I::Item: Sync,
        I::Item: Send,
        I::Item: 'static,
    {
        let items = list.map(|s| (s.get_name(), s));
        SelectView::new().with_all(items)
    }

    let config = siv
        .user_data::<Config>()
        .expect("Config should always be loaded");

    let spaces = crate::actions::load_space_list(config).unwrap();

    let space_select = build_list(spaces.into_iter()).on_submit(on_submit);

    fn on_submit(s: &mut Cursive, space: &Space) {
        let config = s
            .user_data::<Config>()
            .expect("Config should always be loaded to cursive");
        let page_list = crate::actions::load_page_list_for_space(config, &space.id).unwrap();
        let page_select = build_list(page_list.into_iter());
        s.pop_layer();
        s.add_layer(Dialog::around(page_select).title(format!("Pages in {}", &space.name)));
    }

    siv.add_layer(Dialog::around(space_select).title("Spaces"));

    siv.run();
}
