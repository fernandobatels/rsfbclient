//! Firebird remote events API

use crate::Connection;
use rsfbclient_core::{FbError, FirebirdClient};
use std::ops::Fn;
use std::marker::PhantomData;
use std::thread::{self,JoinHandle};

/// Firebird remote events manager
pub struct RemoteEventsManager<'a, C, F>
where
    C: FirebirdClient,
    F: FnMut(&mut Connection<C>) -> Result<(), FbError>,
{
    conn: &'a mut Connection<C>,
    events: Vec<RegisteredEvent<'a, F, C>>
}

impl<'a, C, F> RemoteEventsManager<'a, C, F>
where
    C: FirebirdClient,
    F: FnMut(&mut Connection<C>) -> Result<(), FbError>,
{
    pub fn init(conn: &'a mut Connection<C>) -> Result<Self, FbError> {
        Ok(Self {
            conn,
            events: vec![]
        })
    }

    /// Register a event with a callback
    pub fn listen(&mut self, name: &'a str, closure: F) -> Result<(), FbError>
    {
        self.events.push(RegisteredEvent {
            name,
            phantom: PhantomData,
            closure
        });

        Ok(())
    }

    /// Start the events listners
    pub fn start(&mut self) -> Result<JoinHandle<()>, FbError> {

        let names = self.events.iter()
            .map(|e| e.name.to_string())
            .collect();

        self.conn.que_events(names)?;

        let th = thread::spawn(|| {
            //loop {}
        });

        Ok(th)
    }
}

struct RegisteredEvent<'a, F, C>
    where
        F: FnMut(&mut Connection<C>) -> Result<(), FbError>,
{
    name: &'a str,
    phantom: PhantomData<&'a C>,
    closure: F
}

#[cfg(test)]
mk_tests_default! {
    use crate::*;

    #[test]
    fn remote_events() -> Result<(), FbError> {
        let mut conn = cbuilder().connect()?;

        let mut mn = RemoteEventsManager::init(&mut conn)?;

        let mut counter = 0;

        mn.listen("evento", |c| {

            let (_,): (i32,) = c.query_first(
                "select 1 from rdb$database",
                (),
            )?.unwrap();

            counter = 1;

            Ok(())
        })?;

        let th = mn.start()?;

        conn.execute("execute block as begin POST_EVENT 'evento'; end", ())?;

        th.join().expect("Join thread fail");

        assert_eq!(1, counter);

        Ok(())
    }
}
