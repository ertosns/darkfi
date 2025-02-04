//use std::collections::HashMap;
use tui::widgets::ListState;

#[derive(Clone)]
pub struct NodeIdList {
    pub state: ListState,
    pub node_id: Vec<String>,
}

impl NodeIdList {
    pub fn new(node_id: Vec<String>) -> NodeIdList {
        NodeIdList { state: ListState::default(), node_id }
    }

    pub fn next(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.node_id.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    pub fn previous(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.node_id.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    pub fn unselect(&mut self) {
        self.state.select(None);
    }
}
