//! Firebird remote events API

use crate::{Connection, FirebirdClientFactory};
use rsfbclient_core::{FbError, FirebirdClient};
use std::marker::PhantomData;
use std::thread::{self, JoinHandle};

/// Firebird remote events manager
pub struct RemoteEventsManager<C, F, FA>
where
    C: FirebirdClient + Send + Sync,
    F: FnMut(&mut Connection<C>) -> Result<(), FbError> + Send + Sync + 'static,
    FA: FirebirdClientFactory<C = C> + Send + Sync + Clone + 'static,
{
    events: Vec<RegisteredEvent<F, C>>,
    conn_builder: FA,
}

impl<C, F, FA> RemoteEventsManager<C, F, FA>
where
    C: FirebirdClient + Send + Sync,
    F: FnMut(&mut Connection<C>) -> Result<(), FbError> + Send + Sync + 'static,
    FA: FirebirdClientFactory<C = C> + Send + Sync + Clone + 'static,
{
    pub fn init(fac: FA) -> Result<Self, FbError> {
        Ok(Self {
            events: vec![],
            conn_builder: fac,
        })
    }

    /// Register a event with a callback
    pub fn listen(&mut self, name: String, closure: F) -> Result<(), FbError> {
        self.events.push(RegisteredEvent {
            name,
            phantom: PhantomData,
            closure,
        });

        Ok(())
    }

    /// Start the events listners
    pub fn start(self) -> Result<JoinHandle<Result<(), FbError>>, FbError> {
        let mut threads: Vec<JoinHandle<Result<(), FbError>>> = vec![];

        for event in self.events {
            let cb = self.conn_builder.clone();

            let th = thread::spawn(move || {
                let cli = cb.new_instance()?;
                let mut conn = Connection::open(cli, cb.get_conn_conf())?;

                let mut call = event.closure;
                loop {
                    conn.wait_for_event(event.name.clone())?;

                    call(&mut conn)?;
                }
            });
            threads.push(th);
        }

        let ctl = thread::spawn(|| {
            for th in threads {
                let _ = th
                    .join()
                    .map_err(|_| FbError::from("Join internal thread"))?;
            }

            Ok(())
        });

        Ok(ctl)
    }
}

struct RegisteredEvent<F, C>
where
    F: FnMut(&mut Connection<C>) -> Result<(), FbError> + Send + 'static,
{
    name: String,
    phantom: PhantomData<C>,
    closure: F,
}

#[cfg(test)]
mk_tests_default! {
    use crate::*;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use std::thread;

    #[test]
    #[cfg(all(feature = "linking", not(feature = "embedded_tests")))]
    fn remote_events_native() -> Result<(), FbError> {
        let cb = builder_native()
            .with_dyn_link()
            .with_remote();

        let mut mn = RemoteEventsManager::init(cb)?;

        let counter = Arc::new(Mutex::new(0)).clone();

        let acounter = Arc::clone(&counter);
        mn.listen("evento".to_string(), move |c| {

            let (_,): (i32,) = c.query_first(
                "select 1 from rdb$database",
                (),
            )?.unwrap();

            let mut num = acounter.lock().unwrap();
            *num += 1;

            Ok(())
        })?;

        let _ = mn.start()?;

        thread::sleep(Duration::from_secs(5));

        let mut conn = cbuilder().connect()?;

        conn.execute("execute block as begin POST_EVENT 'evento'; end", ())?;
        thread::sleep(Duration::from_secs(2));
        assert_eq!(1, *counter.lock().unwrap());

        conn.execute("execute block as begin POST_EVENT 'evento'; end", ())?;
        thread::sleep(Duration::from_secs(2));
        assert_eq!(2, *counter.lock().unwrap());

        Ok(())
    }
}
