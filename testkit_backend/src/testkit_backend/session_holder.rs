// Copyright Rouven Bauer
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//    https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::collections::{HashMap, VecDeque};
use std::mem;
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use flume::{Receiver, Sender};
use log::warn;

use neo4j::bookmarks::Bookmarks;
use neo4j::driver::record_stream::{GetSingleRecordError, RecordStream};
use neo4j::driver::transaction::TransactionRecordStream;
use neo4j::driver::{Driver, Record, RoutingControl};
use neo4j::session::{Session, SessionConfig};
use neo4j::summary::Summary;
use neo4j::transaction::TransactionTimeout;
use neo4j::{Neo4jError, ValueSend};

use super::backend_id::Generator;
use super::cypher_value::CypherValues;
use super::driver_holder::EmulatedDriverConfig;
use super::errors::TestKitError;
use super::requests::BackendErrorId;
use super::{BackendId, TestKitResult, TestKitResultT};

#[derive(Debug)]
pub(super) struct SessionHolder {
    tx_req: Sender<Command>,
    rx_res: Receiver<CommandResult>,
    join_handle: Option<JoinHandle<()>>,
}

impl SessionHolder {
    pub(super) fn new(
        id: &BackendId,
        id_generator: Generator,
        driver: Arc<Driver>,
        auto_commit_access_mode: RoutingControl,
        config: SessionConfig,
        emulated_driver_config: Arc<EmulatedDriverConfig>,
    ) -> Self {
        let (tx_req, rx_req) = flume::unbounded();
        let (tx_res, rx_res) = flume::unbounded();
        let handle = thread::Builder::new()
            .name(format!("s-{id}"))
            .spawn(move || {
                let mut runner = SessionHolderRunner {
                    id_generator,
                    auto_commit_access_mode,
                    config,
                    rx_req,
                    tx_res,
                    driver,
                    emulated_driver_config,
                };
                runner.run();
            })
            .unwrap();
        Self {
            tx_req,
            rx_res,
            join_handle: Some(handle),
        }
    }

    pub(super) fn auto_commit(&self, args: AutoCommit) -> AutoCommitResult {
        self.tx_req.send(args.into()).unwrap();
        match self.rx_res.recv().unwrap() {
            CommandResult::AutoCommit(result) => result,
            res => panic!("expected CommandResult::AutoCommit, found {res:?}"),
        }
    }

    pub(super) fn transaction_function(
        &self,
        args: TransactionFunction,
    ) -> TransactionFunctionResult {
        self.tx_req.send(args.into()).unwrap();
        match self.rx_res.recv().unwrap() {
            CommandResult::TransactionFunction(result) => result,
            res => panic!("expected CommandResult::TransactionFunction, found {res:?}"),
        }
    }

    pub(super) fn retryable_positive(&self, args: RetryablePositive) -> RetryablePositiveResult {
        self.tx_req.send(args.into()).unwrap();
        match self.rx_res.recv().unwrap() {
            CommandResult::RetryablePositive(result) => result,
            res => panic!("expected CommandResult::RetryablePositive, found {res:?}"),
        }
    }

    pub(super) fn retryable_negative(&self, args: RetryableNegative) -> RetryableNegativeResult {
        self.tx_req.send(args.into()).unwrap();
        match self.rx_res.recv().unwrap() {
            CommandResult::RetryableNegative(result) => result,
            res => panic!("expected CommandResult::RetryableNegative, found {res:?}"),
        }
    }

    pub(super) fn begin_transaction(&self, args: BeginTransaction) -> BeginTransactionResult {
        self.tx_req.send(args.into()).unwrap();
        match self.rx_res.recv().unwrap() {
            CommandResult::BeginTransaction(result) => result,
            res => panic!("expected CommandResult::BeginTransaction, found {res:?}"),
        }
    }

    pub(super) fn transaction_run(&self, args: TransactionRun) -> TransactionRunResult {
        self.tx_req.send(args.into()).unwrap();
        match self.rx_res.recv().unwrap() {
            CommandResult::TransactionRun(result) => result,
            res => panic!("expected CommandResult::TransactionRun, found {res:?}"),
        }
    }

    pub(super) fn commit_transaction(&self, args: CommitTransaction) -> CommitTransactionResult {
        self.tx_req.send(args.into()).unwrap();
        match self.rx_res.recv().unwrap() {
            CommandResult::CommitTransaction(result) => result,
            res => panic!("expected CommandResult::CommitTransaction, found {res:?}"),
        }
    }

    pub(super) fn rollback_transaction(
        &self,
        args: RollbackTransaction,
    ) -> RollbackTransactionResult {
        self.tx_req.send(args.into()).unwrap();
        match self.rx_res.recv().unwrap() {
            CommandResult::RollbackTransaction(result) => result,
            res => panic!("expected CommandResult::RollbackTransaction, found {res:?}"),
        }
    }

    pub(super) fn close_transaction(&self, args: CloseTransaction) -> CloseTransactionResult {
        self.tx_req.send(args.into()).unwrap();
        match self.rx_res.recv().unwrap() {
            CommandResult::CloseTransaction(result) => result,
            res => panic!("expected CommandResult::CloseTransaction, found {res:?}"),
        }
    }

    pub(super) fn result_next(&self, args: ResultNext) -> ResultNextResult {
        self.tx_req.send(args.into()).unwrap();
        match self.rx_res.recv().unwrap() {
            CommandResult::ResultNext(result) => result,
            res => panic!("expected CommandResult::ResultNext, found {res:?}"),
        }
    }

    pub(super) fn result_single(&self, args: ResultSingle) -> ResultSingleResult {
        self.tx_req.send(args.into()).unwrap();
        match self.rx_res.recv().unwrap() {
            CommandResult::ResultSingle(result) => result,
            res => panic!("expected CommandResult::ResultSingle, found {res:?}"),
        }
    }

    pub(super) fn result_consume(&self, args: ResultConsume) -> ResultConsumeResult {
        self.tx_req.send(args.into()).unwrap();
        match self.rx_res.recv().unwrap() {
            CommandResult::ResultConsume(result) => result,
            res => panic!("expected CommandResult::ResultConsume, found {res:?}"),
        }
    }

    pub(super) fn last_bookmarks(&self, args: LastBookmarks) -> LastBookmarksResult {
        self.tx_req.send(args.into()).unwrap();
        match self.rx_res.recv().unwrap() {
            CommandResult::LastBookmarks(result) => result,
            res => panic!("expected CommandResult::LastBookmarks, found {res:?}"),
        }
    }

    pub(super) fn close(&mut self) -> CloseResult {
        let Some(handle) = self.join_handle.take() else {
            return CloseResult { result: Ok(()) };
        };
        self.tx_req.send(Command::Close).unwrap();
        let res = match self.rx_res.recv().unwrap() {
            CommandResult::Close(result) => result,
            res => panic!("expected CommandResult::Close, found {res:?}"),
        };
        handle.join().unwrap();
        res
    }
}

impl Drop for SessionHolder {
    fn drop(&mut self) {
        if let CloseResult { result: Err(e) } = self.close() {
            warn!("Ignored session closure error on drop: {e:?}");
        }
    }
}

#[derive(Debug)]
struct SessionHolderRunner {
    id_generator: Generator,
    auto_commit_access_mode: RoutingControl,
    config: SessionConfig,
    driver: Arc<Driver>,
    rx_req: Receiver<Command>,
    tx_res: Sender<CommandResult>,
    emulated_driver_config: Arc<EmulatedDriverConfig>,
}

impl SessionHolderRunner {
    fn run(&mut self) {
        let mut session = self.driver.session(self.config.clone());
        let mut record_holders: HashMap<BackendId, RecordBuffer> = Default::default();
        let mut known_transactions: HashMap<BackendId, TxFailState> = Default::default();
        let mut buffered_command = None;
        loop {
            match Self::next_command(&self.rx_req, &mut buffered_command) {
                Command::AutoCommit(AutoCommit {
                    session_id: _,
                    query,
                    params,
                    tx_meta,
                    timeout,
                }) => {
                    let query = Arc::new(query);
                    let params = Arc::new(params);
                    let mut auto_commit = session.auto_commit(&*query);
                    auto_commit = auto_commit.with_routing_control(self.auto_commit_access_mode);
                    if let Some(timeout) = timeout {
                        auto_commit = auto_commit.with_transaction_timeout(timeout);
                    }
                    if let Some(tx_meta) = tx_meta {
                        auto_commit = auto_commit.with_transaction_meta(tx_meta)
                    }
                    let mut started_receiver = false;
                    let known_transactions = &known_transactions;
                    let auto_commit = {
                        let query = Arc::clone(&query);
                        let params = Arc::clone(&params);
                        let started_receiver = &mut started_receiver;
                        let record_holders = &mut record_holders;
                        let tx_res = &self.tx_res;
                        auto_commit.with_receiver(|stream| {
                            *started_receiver = true;
                            let keys = stream.keys();
                            let id = self.id_generator.next_id();
                            record_holders.insert(id, RecordBuffer::new_auto_commit(query, params));
                            tx_res
                                .send(
                                    AutoCommitResult {
                                        result: Ok((id, keys)),
                                    }
                                    .into(),
                                )
                                .unwrap();
                            loop {
                                match Self::next_command(&self.rx_req, &mut buffered_command) {
                                    Command::ResultNext(ResultNext { result_id })
                                        if result_id == id =>
                                    {
                                        self.tx_res
                                            .send(
                                                ResultNextResult {
                                                    result: stream
                                                        .next()
                                                        .map(|r| r.map_err(Into::into).and_then(TryInto::try_into))
                                                        .transpose(),
                                                }
                                                .into(),
                                            )
                                            .unwrap();
                                    }
                                    Command::ResultNext(ResultNext { result_id }) => {
                                        Self::send_record_from_holders(
                                            record_holders,
                                            result_id,
                                            tx_res,
                                        )
                                    }
                                    Command::ResultSingle(ResultSingle { result_id })
                                        if result_id == id =>
                                    {
                                        self.tx_res
                                            .send(
                                                ResultSingleResult {
                                                    result: stream
                                                        .single()
                                                        .map_err(Into::into)
                                                        .and_then(|f| f)
                                                        .map_err(Into::into)
                                                        .and_then(TryInto::try_into),
                                                }
                                                .into(),
                                            )
                                            .unwrap();
                                    }
                                    Command::ResultSingle(ResultSingle { result_id }) => {
                                        Self::send_record_single_from_holders(
                                            record_holders,
                                            result_id,
                                            tx_res,
                                        )
                                    }
                                    Command::ResultConsume(ResultConsume { result_id })
                                        if result_id == id =>
                                    {
                                        record_holders
                                            .get_mut(&id)
                                            .unwrap()
                                            .consume_record_stream(stream);
                                        Self::send_summary_from_holders(record_holders, id, tx_res);
                                        return Ok(());
                                    }
                                    Command::ResultConsume(ResultConsume { result_id }) => {
                                        Self::send_summary_from_holders(
                                            record_holders,
                                            result_id,
                                            tx_res,
                                        )
                                    }
                                    Command::TransactionRun(command) => {
                                        command.default_response(tx_res, known_transactions);
                                    }
                                    Command::CommitTransaction(command) => {
                                        command.default_response(&self.tx_res, known_transactions)
                                    }
                                    Command::RollbackTransaction(command) => {
                                        command.default_response(&self.tx_res, known_transactions)
                                    }
                                    Command::CloseTransaction(command) => {
                                        command.default_response(&self.tx_res, known_transactions)
                                    }
                                    command @ (Command::RetryablePositive(_)
                                    | Command::RetryableNegative(_)) => {
                                        command.reply_error(
                                            tx_res,
                                            TestKitError::backend_err(
                                                "retryable commands not supported outside transaction function",
                                            ),
                                        );
                                    }
                                    command @ (Command::AutoCommit(_)
                                    | Command::TransactionFunction(_)
                                    | Command::BeginTransaction(_)
                                    | Command::LastBookmarks(_)) => {
                                        let _ = buffered_command.insert(command);
                                        record_holders
                                            .get_mut(&id)
                                            .unwrap()
                                            .buffer_record_stream(stream);
                                        return Ok(());
                                    }
                                    command @ Command::Close => {
                                        let _ = buffered_command.insert(command);
                                        record_holders
                                            .get_mut(&id)
                                            .unwrap()
                                            .consume_record_stream(stream);
                                        return Ok(());
                                    }
                                }
                            }
                        })
                    };
                    let res = match &*params {
                        Some(params) => auto_commit.with_parameters(params).run(),
                        None => auto_commit.run(),
                    };
                    if let Err(e) = res {
                        if started_receiver {
                            let command = buffered_command
                                .take()
                                .unwrap_or_else(|| panic!("about to swallow closure error: {e:?}"));
                            command.reply_error(&self.tx_res, e.into());
                            if matches!(command, Command::Close) {
                                return;
                            }
                        } else {
                            self.tx_res
                                .send(
                                    AutoCommitResult {
                                        result: Err(e.into()),
                                    }
                                    .into(),
                                )
                                .unwrap();
                        }
                    }
                }

                Command::BeginTransaction(BeginTransaction {
                    session_id: _,
                    tx_meta,
                    timeout,
                }) => {
                    let last_bookmarks = session.last_bookmarks();
                    let mut transaction = session.transaction();
                    transaction = transaction.with_routing_control(self.auto_commit_access_mode);
                    if let Some(timeout) = timeout {
                        transaction = transaction.with_transaction_timeout(timeout);
                    }
                    if let Some(tx_meta) = tx_meta {
                        transaction = transaction.with_transaction_meta(tx_meta)
                    }
                    let mut started_receiver = false;
                    let res = {
                        let started_receiver = &mut started_receiver;
                        let record_holders = &mut record_holders;
                        let known_transactions = &mut known_transactions;
                        let tx_res = &self.tx_res;
                        transaction.run(|transaction| {
                            *started_receiver = true;
                            let id = self.id_generator.next_id();
                            known_transactions.insert(id, TxFailState::Passed);

                            tx_res
                                .send(BeginTransactionResult { result: Ok(id) }.into())
                                .unwrap();

                            let mut streams: HashMap<BackendId, Option<TransactionRecordStream>> =
                                Default::default();
                            let mut summaries: HashMap<BackendId, Arc<Summary>> =
                                Default::default();
                            let mut queries: HashMap<BackendId, Arc<String>> = Default::default();
                            let mut query_params: HashMap<
                                BackendId,
                                Arc<Option<HashMap<String, ValueSend>>>,
                            > = Default::default();
                            let result = loop {
                                match Self::next_command(&self.rx_req, &mut buffered_command) {
                                    Command::TransactionRun(command) => {
                                        if command.transaction_id != id {
                                            command.default_response(tx_res, known_transactions);
                                            continue;
                                        }
                                        let TransactionRun { params, query, .. } = command;
                                        let query_str = Arc::new(query);
                                        let params = Arc::new(params);
                                        let query = transaction.query(&*query_str);
                                        let result = match &*params {
                                            Some(params) => query.with_parameters(params).run(),
                                            None => query.run(),
                                        };
                                        match result {
                                            Ok(result) => {
                                                let keys = result.keys();
                                                let result_id = self.id_generator.next_id();
                                                streams.insert(result_id, Some(result));
                                                queries.insert(result_id, query_str);
                                                query_params.insert(result_id, params);
                                                let msg = TransactionRunResult {
                                                    result: Ok((result_id, keys)),
                                                };
                                                tx_res.send(msg.into()).unwrap();
                                            }
                                            Err(err) => {
                                                known_transactions.insert(id, TxFailState::Failed);
                                                let msg = TransactionRunResult {
                                                    result: Err(err.into()),
                                                };
                                                tx_res.send(msg.into()).unwrap();
                                            }
                                        }
                                    }
                                    Command::ResultNext(ResultNext { result_id }) => {
                                        match streams.get_mut(&result_id) {
                                            None => Self::send_record_from_holders(
                                                record_holders,
                                                result_id,
                                                tx_res,
                                            ),
                                            Some(stream) => {
                                                let result = match stream {
                                                    None => Err(result_consumed_error()),
                                                    Some(stream) => {
                                                        let res = stream.next();
                                                        if matches!(res, Some(Err(_))) {
                                                            known_transactions
                                                                .insert(id, TxFailState::Failed);
                                                        }
                                                        res.map(|r| r.map_err(Into::into).and_then(TryInto::try_into)).transpose()
                                                    },
                                                };
                                                let msg = ResultNextResult { result };
                                                tx_res.send(msg.into()).unwrap();
                                            }
                                        }
                                    }
                                    Command::ResultSingle(ResultSingle { result_id }) => {
                                        match streams.get_mut(&result_id) {
                                            None => Self::send_record_single_from_holders(
                                                record_holders,
                                                result_id,
                                                tx_res,
                                            ),
                                            Some(stream) => {
                                                let result = match stream {
                                                    None => Err(result_consumed_error()),
                                                    Some(stream) => stream
                                                        .single()
                                                        .map_err(Into::into)
                                                        .and_then(|r| {
                                                            if r.is_err() {
                                                                known_transactions.insert(
                                                                    id,
                                                                    TxFailState::Failed,
                                                                );
                                                            }
                                                            r
                                                        }).map_err(Into::into).and_then(TryInto::try_into),
                                                };
                                                let msg = ResultSingleResult { result };
                                                tx_res.send(msg.into()).unwrap();
                                            }
                                        }
                                    }
                                    Command::ResultConsume(ResultConsume { result_id }) => {
                                        if let Some(stream) = streams.get_mut(&result_id) {
                                            if let Some(stream) = stream.take() {
                                                match stream.consume() {
                                                    Ok(summary) => {
                                                        summaries.insert(
                                                            result_id,
                                                            Arc::new(summary.expect(
                                                                "will only ever fetch summary once",
                                                            )),
                                                        );
                                                    }
                                                    Err(err) => {
                                                        known_transactions
                                                            .insert(id, TxFailState::Failed);
                                                        tx_res
                                                            .send(
                                                                ResultConsumeResult {
                                                                    result: Err(err.into()),
                                                                }
                                                                .into(),
                                                            )
                                                            .unwrap();
                                                        continue;
                                                    }
                                                }
                                            }
                                        }
                                        if let Some(summary) = summaries.get(&result_id) {
                                            let msg = ResultConsumeResult {
                                                result: Ok(SummaryWithQuery {
                                                    summary: Arc::clone(summary),
                                                    query: Arc::clone(
                                                        queries.get(&result_id).unwrap(),
                                                    ),
                                                    parameters: Arc::clone(
                                                        query_params.get(&result_id).unwrap(),
                                                    ),
                                                }),
                                            };
                                            tx_res.send(msg.into()).unwrap();
                                        } else {
                                            Self::send_summary_from_holders(
                                                record_holders,
                                                result_id,
                                                tx_res,
                                            );
                                        }
                                    }
                                    Command::CommitTransaction(command) => {
                                        if command.transaction_id != id {
                                            command.default_response(tx_res, known_transactions);
                                            continue;
                                        }
                                        streams.into_keys().for_each(|id| {
                                            record_holders
                                                .insert(id, RecordBuffer::new_transaction());
                                        });
                                        let result = transaction.commit();
                                        if result.is_err() {
                                            known_transactions.insert(id, TxFailState::Failed);
                                        }
                                        _ = buffered_command.insert(Command::CommitTransaction(command));
                                        return result;
                                    }
                                    Command::RollbackTransaction(command) => {
                                        if command.transaction_id != id {
                                            command.default_response(tx_res, known_transactions);
                                            continue;
                                        }
                                        streams.into_keys().for_each(|id| {
                                            record_holders
                                                .insert(id, RecordBuffer::new_transaction());
                                        });
                                        let result = transaction.rollback();
                                        if result.is_err() {
                                            known_transactions.insert(id, TxFailState::Failed);
                                        }
                                        _ = buffered_command.insert(Command::RollbackTransaction(command));
                                        return result;
                                    }
                                    Command::CloseTransaction(command) => {
                                        if command.transaction_id != id {
                                            command.default_response(tx_res, known_transactions);
                                            continue;
                                        }
                                        streams.into_keys().for_each(|id| {
                                            record_holders
                                                .insert(id, RecordBuffer::new_transaction());
                                        });
                                        let result = transaction.rollback();
                                        if result.is_err() {
                                            known_transactions.insert(id, TxFailState::Failed);
                                        }
                                        _ = buffered_command.insert(Command::CloseTransaction(command));
                                        return result;
                                    }
                                    Command::LastBookmarks(command) => {
                                        command.buffered_response(tx_res, last_bookmarks.clone());
                                    }

                                    command @ (Command::BeginTransaction(_)
                                    | Command::TransactionFunction(_)|Command::AutoCommit(_)) => {
                                        if known_transactions
                                            .get(&id)
                                            .map(|tx| matches!(tx, TxFailState::Failed))
                                            .unwrap_or(false) {
                                            // transaction failed, therefore, we allow to start a new one
                                            _ = buffered_command.insert(command);
                                            return Ok(());
                                        }
                                        command.reply_error(
                                            tx_res, session_already_executing_tx_error(),
                                        )
                                    }
                                    command @ (Command::RetryablePositive(_)
                                    | Command::RetryableNegative(_)) => {
                                        command.reply_error(
                                            tx_res,
                                            TestKitError::backend_err(
                                                "retryable commands not supported outside transaction function",
                                            ),
                                        );
                                    }
                                    command @ (
                                    | Command::Close) => {
                                        _ = buffered_command.insert(command);
                                        break Ok(());
                                    }
                                }
                            };
                            streams.into_keys().for_each(|id| {
                                record_holders.insert(id, RecordBuffer::new_transaction());
                            });
                            result
                        })
                    };
                    let res = res.map_err(Into::into);
                    if !started_receiver {
                        let err = res.expect_err(
                            "unmanaged transaction succeeded without calling the receiver",
                        );

                        self.tx_res
                            .send(BeginTransactionResult { result: Err(err) }.into())
                            .unwrap();
                    } else {
                        let command = buffered_command.take().unwrap_or_else(|| {
                            panic!("left unmanaged transaction without buffered command")
                        });
                        match command {
                            Command::TransactionRun(_) => {
                                let err = res.expect_err("unmanaged transaction left of successful run");
                                self.tx_res.send(TransactionRunResult{ result:Err(err) }.into()).unwrap();
                            }
                            Command::CommitTransaction(command) => {
                                command.real_response(res, &self.tx_res)
                            }

                            Command::RollbackTransaction(command) => {
                                command.real_response(res, &self.tx_res)
                            }

                            Command::CloseTransaction(command) => {
                                command.real_response(res, &self.tx_res)
                            }
                            Command::Close => {
                                self.tx_res
                                    .send(CloseResult { result: res }.into())
                                    .unwrap();
                                return;
                            }
                            command @ (Command::BeginTransaction(_)
                                | Command::TransactionFunction(_)|Command::AutoCommit(_)) => {
                                res.expect("expected failed transaction to not error");
                                _ = buffered_command.insert(command);
                            }
                            _ => panic!(
                                "left transaction function with unexpected buffered command {command:?}"
                            ),
                        }
                    }
                }

                Command::TransactionFunction(TransactionFunction {
                    session_id,
                    tx_meta,
                    timeout,
                    access_mode,
                }) => {
                    let last_bookmarks = session.last_bookmarks();
                    let mut transaction = session.transaction();
                    transaction = transaction.with_routing_control(access_mode);
                    if let Some(timeout) = timeout {
                        transaction = transaction.with_transaction_timeout(timeout);
                    }
                    if let Some(tx_meta) = tx_meta {
                        transaction = transaction.with_transaction_meta(tx_meta)
                    }
                    let _ = buffered_command.insert(
                        TransactionFunction {
                            session_id,
                            tx_meta: Default::default(),
                            timeout,
                            access_mode,
                        }
                        .into(),
                    );
                    let res = {
                        let record_holders = &mut record_holders;
                        let known_transactions = &mut known_transactions;
                        let tx_res = &self.tx_res;
                        transaction.run_with_retry(
                            self.emulated_driver_config.retry_policy(),
                            |transaction| {
                                let id = self.id_generator.next_id();
                                known_transactions.insert(id, TxFailState::Passed);

                                let command = buffered_command.take().unwrap_or_else(|| {
                                    panic!("entered transaction function without buffered command")
                                });
                                let msg = match command {
                                    Command::TransactionFunction(_) => {
                                        TransactionFunctionResult {
                                            result: Ok(RetryableOutcome::Retry(id)),
                                        }.into()
                                    }
                                    Command::RetryableNegative(_) => {
                                        RetryableNegativeResult {
                                            result: Ok(RetryableOutcome::Retry(id)),
                                        }.into()
                                    }
                                    Command::RetryablePositive(_) => {
                                        RetryablePositiveResult {
                                            result: Ok(RetryableOutcome::Retry(id)),
                                        }.into()
                                    }
                                    _ => panic!(
                                        "entered transaction function with unexpected buffered command {command:?}"
                                    ),
                                };
                                tx_res
                                    .send(msg)
                                    .unwrap();

                                let mut streams: HashMap<BackendId, Option<TransactionRecordStream>> =
                                    Default::default();
                                let mut summaries: HashMap<BackendId, Arc<Summary>> =
                                    Default::default();
                                let mut queries: HashMap<BackendId, Arc<String>> = Default::default();
                                let mut query_params: HashMap<
                                    BackendId,
                                    Arc<Option<HashMap<String, ValueSend>>>,
                                > = Default::default();
                                let mut state_tracker = TransactionFunctionStateTracker::new(&self.id_generator);
                                loop {
                                    match Self::next_command(&self.rx_req, &mut buffered_command) {
                                        Command::TransactionRun(command) => {
                                            if command.transaction_id != id {
                                                let error = state_tracker.wrap_testkit_error(command.scope_error(known_transactions));
                                                tx_res.send(
                                                    TransactionRunResult {
                                                        result: Err(error)
                                                    }.into()).unwrap();
                                                continue;
                                            }
                                            if state_tracker.failed() {
                                                Command::TransactionRun(command).reply_error(
                                                    tx_res,
                                                    state_tracker.wrap_testkit_error(transaction_out_of_scope_error())
                                                );
                                                continue;
                                            }
                                            let TransactionRun { params, query, .. } = command;
                                            let query_str = Arc::new(query);
                                            let params = Arc::new(params);
                                            let query = transaction.query(&*query_str);
                                            let result = match &*params {
                                                Some(params) => query.with_parameters(params).run(),
                                                None => query.run(),
                                            };
                                            match result {
                                                Ok(result) => {
                                                    let keys = result.keys();
                                                    let result_id = self.id_generator.next_id();
                                                    streams.insert(result_id, Some(result));
                                                    queries.insert(result_id, query_str);
                                                    query_params.insert(result_id, params);
                                                    let msg = TransactionRunResult {
                                                        result: Ok((result_id, keys)),
                                                    };
                                                    tx_res.send(msg.into()).unwrap();
                                                }
                                                Err(err) => {
                                                    known_transactions.insert(id, TxFailState::Failed);
                                                    let msg = TransactionRunResult {
                                                        result: Err(state_tracker.wrap_neo4j_error(err)),
                                                    };
                                                    tx_res.send(msg.into()).unwrap();
                                                }
                                            }
                                        }
                                        Command::ResultNext(ResultNext { result_id }) => {
                                            match streams.get_mut(&result_id) {
                                                None => Self::send_record_from_holders(
                                                    record_holders,
                                                    result_id,
                                                    tx_res,
                                                ),
                                                Some(stream) => {
                                                    let result = match stream {
                                                        None => Err(result_consumed_error()),
                                                        Some(stream) => {
                                                            if state_tracker.failed() {
                                                                Err(state_tracker.wrap_testkit_error(transaction_out_of_scope_error()))
                                                            } else {
                                                                let res = stream.next();
                                                                res.transpose().map_err(|e| {
                                                                    known_transactions
                                                                        .insert(id, TxFailState::Failed);
                                                                    state_tracker.wrap_neo4j_error(e)
                                                                }).and_then(|r| r.map(TryInto::try_into).transpose().map_err(|e| {
                                                                    state_tracker.wrap_testkit_error(e)
                                                                }))
                                                            }
                                                        },
                                                    };
                                                    let msg = ResultNextResult { result };
                                                    tx_res.send(msg.into()).unwrap();
                                                }
                                            }
                                        }
                                        Command::ResultSingle(ResultSingle { result_id }) => {
                                            match streams.get_mut(&result_id) {
                                                None => Self::send_record_single_from_holders(
                                                    record_holders,
                                                    result_id,
                                                    tx_res,
                                                ),
                                                Some(stream) => {
                                                    let result = match stream {
                                                        None => Err(result_consumed_error()),
                                                        Some(stream) => {

                                                            if state_tracker.failed() {
                                                                Err(state_tracker.wrap_testkit_error(transaction_out_of_scope_error()))
                                                            } else {
                                                                stream
                                                                    .single()
                                                                    .map_err(|e|state_tracker.wrap_neo4j_error(e.into()))
                                                                    .and_then(|r| {
                                                                        r.map_err(|e| {
                                                                            known_transactions.insert(
                                                                                id,
                                                                                TxFailState::Failed,
                                                                            );
                                                                            state_tracker.wrap_neo4j_error(e)
                                                                        })
                                                                    })
                                                                    .and_then(|r| {
                                                                        r.try_into().map_err(|e| {
                                                                            state_tracker.wrap_testkit_error(e)
                                                                        })
                                                                    })
                                                            }
                                                        },
                                                    };
                                                    let msg = ResultSingleResult { result };
                                                    tx_res.send(msg.into()).unwrap();
                                                }
                                            }
                                        }
                                        Command::ResultConsume(ResultConsume { result_id }) => {
                                            if let Some(stream) = streams.get_mut(&result_id) {
                                                if state_tracker.failed() {
                                                    tx_res
                                                        .send(
                                                            ResultConsumeResult {
                                                                result: Err(transaction_out_of_scope_error()),
                                                            }
                                                                .into(),
                                                        )
                                                        .unwrap();
                                                    continue;
                                                }
                                                if let Some(stream) = stream.take() {
                                                    match stream.consume() {
                                                        Ok(summary) => {
                                                            summaries.insert(
                                                                result_id,
                                                                Arc::new(summary.expect(
                                                                    "will only ever fetch summary once",
                                                                )),
                                                            );
                                                        }
                                                        Err(err) => {
                                                            known_transactions
                                                                .insert(id, TxFailState::Failed);
                                                            tx_res
                                                                .send(
                                                                    ResultConsumeResult {
                                                                        result: Err(state_tracker.wrap_neo4j_error(err)),
                                                                    }
                                                                        .into(),
                                                                )
                                                                .unwrap();
                                                            continue;
                                                        }
                                                    }
                                                }
                                            }
                                            if let Some(summary) = summaries.get(&result_id) {
                                                let msg = ResultConsumeResult {
                                                    result: Ok(SummaryWithQuery {
                                                        summary: Arc::clone(summary),
                                                        query: Arc::clone(
                                                            queries.get(&result_id).unwrap(),
                                                        ),
                                                        parameters: Arc::clone(
                                                            query_params.get(&result_id).unwrap(),
                                                        ),
                                                    }),
                                                };
                                                tx_res.send(msg.into()).unwrap();
                                            } else {
                                                Self::send_summary_from_holders(
                                                    record_holders,
                                                    result_id,
                                                    tx_res,
                                                );
                                            }
                                        }
                                        Command::CommitTransaction(command) => {
                                            if command.transaction_id != id {
                                                command.default_response(tx_res, known_transactions);
                                                continue;
                                            }
                                            Command::CommitTransaction(command).reply_error(
                                                tx_res, TestKitError::backend_err(
                                                    "explicit commit not supported in transaction function",
                                                ),
                                            );
                                        }
                                        Command::RollbackTransaction(command) => {
                                            if command.transaction_id != id {
                                                command.default_response(tx_res, known_transactions);
                                                continue;
                                            }
                                            Command::RollbackTransaction(command).reply_error(
                                                tx_res, TestKitError::backend_err(
                                                    "explicit rollback not supported in transaction function",
                                                ),
                                            );
                                        }
                                        Command::CloseTransaction(command) => {
                                            if command.transaction_id != id {
                                                command.default_response(tx_res, known_transactions);
                                                continue;
                                            }
                                            Command::CloseTransaction(command).reply_error(
                                                tx_res, TestKitError::backend_err(
                                                    "explicit close not supported in transaction function",
                                                ),
                                            );
                                        }
                                        Command::LastBookmarks(command) => {
                                            command.buffered_response(tx_res, last_bookmarks.clone());
                                        }
                                        command @ (Command::BeginTransaction(_)
                                        | Command::TransactionFunction(_)
                                        | Command::AutoCommit(_)) => command.reply_error(
                                            tx_res, session_already_executing_tx_error()
                                        ),
                                        command @ Command::RetryablePositive(_) => {
                                            streams.into_keys().for_each(|id| {
                                                record_holders
                                                    .insert(id, RecordBuffer::new_transaction());
                                            });
                                            let result = transaction.commit();
                                            if result.is_err() {
                                                known_transactions.insert(id, TxFailState::Failed);
                                            }
                                            let _ = buffered_command.insert(command);
                                            return result.map(|_| Ok(()));
                                        }
                                        Command::RetryableNegative(command) => {
                                            streams.into_keys().for_each(|id| {
                                                record_holders
                                                    .insert(id, RecordBuffer::new_transaction());
                                            });
                                            let error_id = command.error_id;
                                            let _ = buffered_command.insert(Command::RetryableNegative(command));
                                            return match error_id {
                                                BackendErrorId::BackendError(id) => {
                                                    match state_tracker.into_error(id) {
                                                        TransactionFunctionError::TestKit(e) => {
                                                            Ok(Err(e))
                                                        }
                                                        TransactionFunctionError::Neo4j(e) => Err(e)
                                                    }
                                                }
                                                BackendErrorId::ClientError => {
                                                    Ok(Err(
                                                        TestKitError::FrontendError {
                                                            msg: String::from("Client said no!"),
                                                        },
                                                    ))
                                                }
                                            };
                                        }
                                        command @ (
                                            | Command::Close) => {
                                            let _ = buffered_command.insert(command);

                                            streams.into_keys().for_each(|id| {
                                                record_holders.insert(id, RecordBuffer::new_transaction());
                                            });
                                            return Ok(Ok(()));
                                        }
                                    }
                                }
                            }
                        )
                    };
                    let command = buffered_command.take().unwrap_or_else(|| {
                        panic!("left transaction function without buffered command")
                    });
                    let res = res.map_err(Into::into).and_then(|x| x);
                    match command {
                        Command::RetryablePositive(_) => {
                            self.tx_res
                                .send(
                                    RetryablePositiveResult {
                                        result: res.map(|_| RetryableOutcome::Done),
                                    }
                                    .into(),
                                )
                                .unwrap();
                        }
                        Command::RetryableNegative(_) => {
                            self.tx_res
                                .send(
                                    RetryableNegativeResult {
                                        result: res.map(|_| RetryableOutcome::Done),
                                    }
                                    .into(),
                                )
                                .unwrap();
                        }
                        Command::TransactionFunction(_) => {
                            self.tx_res
                                .send(
                                    TransactionFunctionResult {
                                        result: res.map(|_| RetryableOutcome::Done),
                                    }
                                    .into(),
                                )
                                .unwrap();
                        }
                        Command::Close => {
                            self.tx_res
                                .send(CloseResult { result: res }.into())
                                .unwrap();
                            return;
                        }
                        _ => panic!(
                            "left transaction function with unexpected buffered command {command:?}"
                        ),
                    }
                }

                command @ (Command::RetryableNegative(_) | Command::RetryablePositive(_)) => {
                    command.reply_error(
                        &self.tx_res,
                        TestKitError::backend_err(
                            "retryable commands not supported outside transaction function",
                        ),
                    );
                }

                Command::TransactionRun(command) => {
                    command.default_response(&self.tx_res, &known_transactions)
                }

                Command::CommitTransaction(command) => {
                    command.default_response(&self.tx_res, &known_transactions)
                }

                Command::RollbackTransaction(command) => {
                    command.default_response(&self.tx_res, &known_transactions)
                }

                Command::CloseTransaction(command) => {
                    command.default_response(&self.tx_res, &known_transactions)
                }

                Command::ResultNext(ResultNext { result_id }) => {
                    Self::send_record_from_holders(&mut record_holders, result_id, &self.tx_res)
                }

                Command::ResultSingle(ResultSingle { result_id }) => {
                    Self::send_record_single_from_holders(
                        &mut record_holders,
                        result_id,
                        &self.tx_res,
                    )
                }

                Command::ResultConsume(ResultConsume { result_id }) => {
                    Self::send_summary_from_holders(&mut record_holders, result_id, &self.tx_res)
                }

                Command::LastBookmarks(command) => command.real_response(&self.tx_res, &session),

                Command::Close => {
                    self.tx_res
                        .send(CloseResult { result: Ok(()) }.into())
                        .unwrap();
                    return;
                }
            }
        }
    }

    fn send_record_from_holders(
        record_holders: &mut HashMap<BackendId, RecordBuffer>,
        id: BackendId,
        tx_res: &Sender<CommandResult>,
    ) {
        let msg = match record_holders.get_mut(&id) {
            None => ResultNextResult {
                result: Err(TestKitError::backend_err(format!(
                    "unknown result id {id} in session"
                ))),
            }
            .into(),
            Some(buffer) => ResultNextResult {
                result: buffer.next(),
            }
            .into(),
        };
        tx_res.send(msg).unwrap();
    }

    fn send_record_single_from_holders(
        record_holders: &mut HashMap<BackendId, RecordBuffer>,
        id: BackendId,
        tx_res: &Sender<CommandResult>,
    ) {
        let msg = match record_holders.get_mut(&id) {
            None => ResultSingleResult {
                result: Err(TestKitError::backend_err(format!(
                    "unknown result id {id} in session"
                ))),
            }
            .into(),
            Some(buffer) => ResultSingleResult {
                result: buffer.single(),
            }
            .into(),
        };
        tx_res.send(msg).unwrap();
    }

    fn send_summary_from_holders(
        record_holders: &mut HashMap<BackendId, RecordBuffer>,
        id: BackendId,
        tx_res: &Sender<CommandResult>,
    ) {
        let msg = match record_holders.get_mut(&id) {
            None => ResultConsumeResult {
                result: Err(TestKitError::backend_err(format!(
                    "unknown result id {id} in session"
                ))),
            }
            .into(),
            Some(buffer) => ResultConsumeResult {
                result: buffer.consume(),
            }
            .into(),
        };
        tx_res.send(msg).unwrap();
    }

    fn next_command(rx_req: &Receiver<Command>, buffered_command: &mut Option<Command>) -> Command {
        let _ = buffered_command.get_or_insert_with(|| rx_req.recv().unwrap());
        buffered_command.take().unwrap()
    }
}

fn result_out_of_scope_error() -> TestKitError {
    TestKitError::driver_error_client_only(
        String::from("ResultOutOfScope"),
        String::from("The record stream's transaction has been closed"),
        false,
    )
}

fn result_consumed_error() -> TestKitError {
    TestKitError::driver_error_client_only(
        String::from("ResultConsumed"),
        String::from("The record stream has been consumed"),
        false,
    )
}

fn transaction_out_of_scope_error() -> TestKitError {
    TestKitError::driver_error_client_only(
        String::from("TransactionOutOfScope"),
        String::from("The transaction has been closed"),
        false,
    )
}

fn session_already_executing_tx_error() -> TestKitError {
    TestKitError::driver_error_client_only(
        String::from("SessionError"),
        String::from("Session is already executing a transaction"),
        false,
    )
}

#[derive(Debug)]
enum RecordBuffer {
    AutoCommit {
        records: VecDeque<Result<Record, Arc<Neo4jError>>>,
        summary: Option<Arc<Summary>>,
        query: Arc<String>,
        params: Arc<Option<HashMap<String, ValueSend>>>,
        consumed: bool,
    },
    Transaction,
}

impl RecordBuffer {
    fn new_auto_commit(
        query: Arc<String>,
        params: Arc<Option<HashMap<String, ValueSend>>>,
    ) -> Self {
        RecordBuffer::AutoCommit {
            records: Default::default(),
            summary: Default::default(),
            query,
            params,
            consumed: false,
        }
    }

    fn new_transaction() -> Self {
        RecordBuffer::Transaction
    }

    fn buffer_record_stream(&mut self, stream: &mut RecordStream) {
        match self {
            RecordBuffer::AutoCommit {
                records, summary, ..
            } => {
                stream.for_each(|rec| records.push_back(rec.map_err(Arc::new)));
                *summary = stream
                    .consume()
                    .expect("result stream exhausted above => cannot fail on consume")
                    .map(Arc::new);
            }
            RecordBuffer::Transaction => {
                panic!("cannot buffer record stream in transaction")
            }
        }
    }

    fn consume_record_stream(&mut self, stream: &mut RecordStream) {
        match self {
            RecordBuffer::AutoCommit {
                records,
                summary: buffer_summary,
                ..
            } => {
                let dropped_records = Self::drop_buffered_records(records);
                if dropped_records.is_err() {
                    return;
                }
                match stream.consume() {
                    Ok(summary) => *buffer_summary = summary.map(Arc::new),
                    Err(e) => records.push_back(Err(Arc::new(e))),
                }
            }
            RecordBuffer::Transaction => {
                panic!("cannot consume record stream in transaction")
            }
        }
    }

    fn next(&mut self) -> TestKitResultT<Option<CypherValues>> {
        match self {
            RecordBuffer::AutoCommit {
                records, consumed, ..
            } => {
                if *consumed {
                    Err(result_consumed_error())
                } else {
                    records
                        .pop_front()
                        .transpose()
                        .map_err(|e| TestKitError::clone_neo4j_error(&e))
                        .and_then(|r| r.map(TryInto::try_into).transpose())
                }
            }
            RecordBuffer::Transaction => Err(result_out_of_scope_error()),
        }
    }

    fn single(&mut self) -> TestKitResultT<CypherValues> {
        match self {
            RecordBuffer::AutoCommit {
                records, consumed, ..
            } => {
                if *consumed {
                    Err(result_consumed_error())
                } else {
                    *consumed = true;
                    match records.pop_front() {
                        None => Err(TestKitError::from(Neo4jError::from(
                            GetSingleRecordError::NoRecords,
                        ))),
                        Some(result) => match result {
                            Ok(record) => {
                                if !records.is_empty() {
                                    match Self::drop_buffered_records(records) {
                                        Err(e) => Err(e),
                                        Ok(()) => Err(TestKitError::from(Neo4jError::from(
                                            GetSingleRecordError::TooManyRecords,
                                        ))),
                                    }
                                } else {
                                    record.try_into()
                                }
                            }
                            Err(e) => Err(TestKitError::clone_neo4j_error(&e)),
                        },
                    }
                }
            }
            RecordBuffer::Transaction => Err(result_out_of_scope_error()),
        }
    }

    fn consume(&mut self) -> TestKitResultT<SummaryWithQuery> {
        match self {
            RecordBuffer::AutoCommit {
                records,
                summary,
                consumed,
                query,
                params,
            } => {
                *consumed = true;
                Self::drop_buffered_records(records)?;
                match summary {
                    None => Err(TestKitError::backend_err(
                        "cannot receive summary of a failed record stream",
                    )),
                    Some(summary) => Ok(SummaryWithQuery {
                        summary: Arc::clone(summary),
                        query: Arc::clone(query),
                        parameters: Arc::clone(params),
                    }),
                }
            }
            RecordBuffer::Transaction => Err(result_out_of_scope_error()),
        }
    }

    fn drop_buffered_records(
        records: &mut VecDeque<Result<Record, Arc<Neo4jError>>>,
    ) -> TestKitResult {
        let mut swapped_records = Default::default();
        mem::swap(records, &mut swapped_records);
        let dropped_records = swapped_records.into_iter().try_for_each(|r| {
            match r {
                Ok(r) => drop(r),
                Err(e) => return Err(e),
            }
            Ok(())
        });
        if let Err(e) = dropped_records {
            records.push_back(Err(Arc::clone(&e)));
            return Err(TestKitError::clone_neo4j_error(&e));
        }
        Ok(())
    }
}

#[derive(Debug)]
enum TxFailState {
    Failed,
    Passed,
}

#[derive(Debug)]
struct TransactionFunctionStateTracker<'gen> {
    state: TransactionFunctionState,
    errors: HashMap<BackendId, TransactionFunctionError>,
    generator: &'gen Generator,
}

#[derive(Debug)]
enum TransactionFunctionState {
    Ok,
    Err,
}

#[derive(Debug)]
enum TransactionFunctionError {
    TestKit(TestKitError),
    Neo4j(Neo4jError),
}

impl<'gen> TransactionFunctionStateTracker<'gen> {
    fn new(generator: &'gen Generator) -> Self {
        Self {
            state: TransactionFunctionState::Ok,
            errors: Default::default(),
            generator,
        }
    }

    fn wrap_neo4j_error(&mut self, error: Neo4jError) -> TestKitError {
        self.state = TransactionFunctionState::Err;
        let id = self.generator.next_id();
        let mut testkit_error = TestKitError::clone_neo4j_error(&error);
        testkit_error.set_id(id);
        self.errors
            .insert(id, TransactionFunctionError::Neo4j(error));
        testkit_error
    }

    fn wrap_testkit_error(&mut self, mut error: TestKitError) -> TestKitError {
        if error.get_id().is_none() {
            error.set_id_gen(self.generator);
        }
        if let Some(id) = error.get_id() {
            self.errors
                .insert(id, TransactionFunctionError::TestKit(error.clone()));
        }
        error
    }

    fn failed(&self) -> bool {
        matches!(self.state, TransactionFunctionState::Err)
    }

    fn into_error(mut self, id: BackendId) -> TransactionFunctionError {
        self.errors.remove(&id).unwrap_or_else(|| {
            TransactionFunctionError::TestKit(TestKitError::backend_err(format!(
                "Unknown error with id {id} found"
            )))
        })
    }
}

#[derive(Debug)]
pub(super) enum Command {
    AutoCommit(AutoCommit),
    BeginTransaction(BeginTransaction),
    TransactionFunction(TransactionFunction),
    #[allow(dead_code)] // for symmetry with driver holder
    RetryablePositive(RetryablePositive),
    RetryableNegative(RetryableNegative),
    TransactionRun(TransactionRun),
    CommitTransaction(CommitTransaction),
    RollbackTransaction(RollbackTransaction),
    CloseTransaction(CloseTransaction),
    ResultNext(ResultNext),
    ResultSingle(ResultSingle),
    ResultConsume(ResultConsume),
    LastBookmarks(LastBookmarks),
    Close,
}

#[derive(Debug)]
pub(super) enum CommandResult {
    AutoCommit(AutoCommitResult),
    BeginTransaction(BeginTransactionResult),
    TransactionFunction(TransactionFunctionResult),
    RetryablePositive(RetryablePositiveResult),
    RetryableNegative(RetryableNegativeResult),
    TransactionRun(TransactionRunResult),
    CommitTransaction(CommitTransactionResult),
    RollbackTransaction(RollbackTransactionResult),
    CloseTransaction(CloseTransactionResult),
    ResultNext(ResultNextResult),
    ResultSingle(ResultSingleResult),
    ResultConsume(ResultConsumeResult),
    LastBookmarks(LastBookmarksResult),
    Close(CloseResult),
}

impl Command {
    fn reply_error(&self, tx_res: &Sender<CommandResult>, err: TestKitError) {
        let msg = match self {
            Command::AutoCommit(_) => AutoCommitResult { result: Err(err) }.into(),
            Command::BeginTransaction(_) => BeginTransactionResult { result: Err(err) }.into(),
            Command::TransactionFunction(_) => {
                TransactionFunctionResult { result: Err(err) }.into()
            }
            Command::RetryablePositive(_) => RetryablePositiveResult { result: Err(err) }.into(),
            Command::RetryableNegative(_) => RetryableNegativeResult { result: Err(err) }.into(),
            Command::TransactionRun(_) => TransactionRunResult { result: Err(err) }.into(),
            Command::CommitTransaction(_) => CommitTransactionResult { result: Err(err) }.into(),
            Command::RollbackTransaction(_) => {
                RollbackTransactionResult { result: Err(err) }.into()
            }
            Command::CloseTransaction(_) => CloseTransactionResult { result: Err(err) }.into(),
            Command::ResultNext(_) => ResultNextResult { result: Err(err) }.into(),
            Command::ResultSingle(_) => ResultSingleResult { result: Err(err) }.into(),
            Command::ResultConsume(_) => ResultConsumeResult { result: Err(err) }.into(),
            Command::LastBookmarks(_) => LastBookmarksResult { result: Err(err) }.into(),
            Command::Close => CloseResult { result: Err(err) }.into(),
        };
        tx_res.send(msg).unwrap();
    }
}

#[derive(Debug)]
pub(super) struct AutoCommit {
    pub(super) session_id: BackendId,
    pub(super) query: String,
    pub(super) params: Option<HashMap<String, ValueSend>>,
    pub(super) tx_meta: Option<HashMap<String, ValueSend>>,
    pub(super) timeout: Option<TransactionTimeout>,
}

impl From<AutoCommit> for Command {
    fn from(c: AutoCommit) -> Self {
        Command::AutoCommit(c)
    }
}

#[must_use]
#[derive(Debug)]
pub(super) struct AutoCommitResult {
    pub(super) result: TestKitResultT<(BackendId, RecordKeys)>,
}

impl From<AutoCommitResult> for CommandResult {
    fn from(r: AutoCommitResult) -> Self {
        CommandResult::AutoCommit(r)
    }
}

#[derive(Debug)]
pub(super) struct TransactionFunction {
    pub(super) session_id: BackendId,
    pub(super) tx_meta: Option<HashMap<String, ValueSend>>,
    pub(super) timeout: Option<TransactionTimeout>,
    pub(super) access_mode: RoutingControl,
}

impl From<TransactionFunction> for Command {
    fn from(c: TransactionFunction) -> Self {
        Command::TransactionFunction(c)
    }
}

#[must_use]
#[derive(Debug)]
pub(super) struct TransactionFunctionResult {
    pub(super) result: TestKitResultT<RetryableOutcome>,
}

impl From<TransactionFunctionResult> for CommandResult {
    fn from(r: TransactionFunctionResult) -> Self {
        CommandResult::TransactionFunction(r)
    }
}

#[derive(Debug)]
pub(super) struct RetryablePositive {
    pub(super) session_id: BackendId,
}

impl From<RetryablePositive> for Command {
    fn from(c: RetryablePositive) -> Self {
        Command::RetryablePositive(c)
    }
}

#[must_use]
#[derive(Debug)]
pub(super) struct RetryablePositiveResult {
    pub(super) result: TestKitResultT<RetryableOutcome>,
}

impl From<RetryablePositiveResult> for CommandResult {
    fn from(r: RetryablePositiveResult) -> Self {
        CommandResult::RetryablePositive(r)
    }
}

#[derive(Debug)]
pub(super) struct RetryableNegative {
    pub(super) session_id: BackendId,
    pub(super) error_id: BackendErrorId,
}

impl From<RetryableNegative> for Command {
    fn from(c: RetryableNegative) -> Self {
        Command::RetryableNegative(c)
    }
}

#[must_use]
#[derive(Debug)]
pub(super) struct RetryableNegativeResult {
    pub(super) result: TestKitResultT<RetryableOutcome>,
}

#[derive(Debug)]
pub(super) enum RetryableOutcome {
    Retry(BackendId),
    Done,
}

impl From<RetryableNegativeResult> for CommandResult {
    fn from(r: RetryableNegativeResult) -> Self {
        CommandResult::RetryableNegative(r)
    }
}

#[derive(Debug)]
pub(super) struct BeginTransaction {
    pub(super) session_id: BackendId,
    pub(super) tx_meta: Option<HashMap<String, ValueSend>>,
    pub(super) timeout: Option<TransactionTimeout>,
}

impl From<BeginTransaction> for Command {
    fn from(c: BeginTransaction) -> Self {
        Command::BeginTransaction(c)
    }
}

#[must_use]
#[derive(Debug)]
pub(super) struct BeginTransactionResult {
    pub(super) result: TestKitResultT<BackendId>,
}

impl From<BeginTransactionResult> for CommandResult {
    fn from(r: BeginTransactionResult) -> Self {
        CommandResult::BeginTransaction(r)
    }
}

#[derive(Debug)]
pub(super) struct TransactionRun {
    pub(super) transaction_id: BackendId,
    pub(super) query: String,
    pub(super) params: Option<HashMap<String, ValueSend>>,
}

impl From<TransactionRun> for Command {
    fn from(c: TransactionRun) -> Self {
        Command::TransactionRun(c)
    }
}

impl TransactionRun {
    fn default_response(
        &self,
        tx_res: &Sender<CommandResult>,
        known_transactions: &HashMap<BackendId, TxFailState>,
    ) {
        let err = self.scope_error(known_transactions);
        let msg = CommandResult::TransactionRun(TransactionRunResult { result: Err(err) });
        tx_res.send(msg).unwrap();
    }

    fn scope_error(&self, known_transactions: &HashMap<BackendId, TxFailState>) -> TestKitError {
        match known_transactions.get(&self.transaction_id) {
            Some(_) => transaction_out_of_scope_error(),
            None => TestKitError::backend_err("transaction not found"),
        }
    }
}

#[must_use]
#[derive(Debug)]
pub(super) struct TransactionRunResult {
    pub(super) result: TestKitResultT<(BackendId, RecordKeys)>,
}

impl From<TransactionRunResult> for CommandResult {
    fn from(r: TransactionRunResult) -> Self {
        CommandResult::TransactionRun(r)
    }
}

#[derive(Debug)]
pub(super) struct CommitTransaction {
    pub(super) transaction_id: BackendId,
}

impl From<CommitTransaction> for Command {
    fn from(c: CommitTransaction) -> Self {
        Command::CommitTransaction(c)
    }
}

impl CommitTransaction {
    fn default_response(
        &self,
        tx_res: &Sender<CommandResult>,
        known_transactions: &HashMap<BackendId, TxFailState>,
    ) {
        let err = match known_transactions.get(&self.transaction_id) {
            Some(_) => transaction_out_of_scope_error(),
            None => TestKitError::backend_err("transaction not found"),
        };
        let msg = CommandResult::CommitTransaction(CommitTransactionResult { result: Err(err) });
        tx_res.send(msg).unwrap();
    }

    fn real_response(&self, result: TestKitResult, tx_res: &Sender<CommandResult>) {
        let msg = CommandResult::CommitTransaction(CommitTransactionResult { result });
        tx_res.send(msg).unwrap();
    }
}

#[must_use]
#[derive(Debug)]
pub(super) struct CommitTransactionResult {
    pub(super) result: TestKitResult,
}

impl From<CommitTransactionResult> for CommandResult {
    fn from(r: CommitTransactionResult) -> Self {
        CommandResult::CommitTransaction(r)
    }
}

#[derive(Debug)]
pub(super) struct RollbackTransaction {
    pub(super) transaction_id: BackendId,
}

impl From<RollbackTransaction> for Command {
    fn from(c: RollbackTransaction) -> Self {
        Command::RollbackTransaction(c)
    }
}

impl RollbackTransaction {
    fn default_response(
        &self,
        tx_res: &Sender<CommandResult>,
        known_transactions: &HashMap<BackendId, TxFailState>,
    ) {
        let result = match known_transactions.get(&self.transaction_id) {
            Some(TxFailState::Passed) => Err(transaction_out_of_scope_error()),
            Some(TxFailState::Failed) => Ok(()),
            None => Err(TestKitError::backend_err("transaction not found")),
        };
        let msg = CommandResult::RollbackTransaction(RollbackTransactionResult { result });
        tx_res.send(msg).unwrap();
    }

    fn real_response(&self, result: TestKitResult, tx_res: &Sender<CommandResult>) {
        let msg = CommandResult::RollbackTransaction(RollbackTransactionResult { result });
        tx_res.send(msg).unwrap();
    }
}

#[must_use]
#[derive(Debug)]
pub(super) struct RollbackTransactionResult {
    pub(super) result: TestKitResult,
}

impl From<RollbackTransactionResult> for CommandResult {
    fn from(r: RollbackTransactionResult) -> Self {
        CommandResult::RollbackTransaction(r)
    }
}

#[derive(Debug)]
pub(super) struct CloseTransaction {
    pub(super) transaction_id: BackendId,
}

impl From<CloseTransaction> for Command {
    fn from(c: CloseTransaction) -> Self {
        Command::CloseTransaction(c)
    }
}

impl CloseTransaction {
    fn default_response(
        &self,
        tx_res: &Sender<CommandResult>,
        known_transactions: &HashMap<BackendId, TxFailState>,
    ) {
        let result = match known_transactions.get(&self.transaction_id) {
            Some(_) => Ok(()),
            None => Err(TestKitError::backend_err("transaction not found")),
        };
        let msg = CommandResult::CloseTransaction(CloseTransactionResult { result });
        tx_res.send(msg).unwrap();
    }

    fn real_response(&self, result: TestKitResult, tx_res: &Sender<CommandResult>) {
        let msg = CommandResult::CloseTransaction(CloseTransactionResult { result });
        tx_res.send(msg).unwrap();
    }
}

#[must_use]
#[derive(Debug)]
pub(super) struct CloseTransactionResult {
    pub(super) result: TestKitResult,
}

impl From<CloseTransactionResult> for CommandResult {
    fn from(r: CloseTransactionResult) -> Self {
        CommandResult::CloseTransaction(r)
    }
}

#[derive(Debug)]
pub(super) struct ResultNext {
    pub(super) result_id: BackendId,
}

impl From<ResultNext> for Command {
    fn from(c: ResultNext) -> Self {
        Command::ResultNext(c)
    }
}

#[must_use]
#[derive(Debug)]
pub(super) struct ResultNextResult {
    pub(super) result: TestKitResultT<Option<CypherValues>>,
}

impl From<ResultNextResult> for CommandResult {
    fn from(r: ResultNextResult) -> Self {
        CommandResult::ResultNext(r)
    }
}

#[derive(Debug)]
pub(super) struct ResultSingle {
    pub(super) result_id: BackendId,
}

impl From<ResultSingle> for Command {
    fn from(c: ResultSingle) -> Self {
        Command::ResultSingle(c)
    }
}

#[must_use]
#[derive(Debug)]
pub(super) struct ResultSingleResult {
    pub(super) result: TestKitResultT<CypherValues>,
}

impl From<ResultSingleResult> for CommandResult {
    fn from(r: ResultSingleResult) -> Self {
        CommandResult::ResultSingle(r)
    }
}

#[derive(Debug)]
pub(super) struct ResultConsume {
    pub(super) result_id: BackendId,
}

impl From<ResultConsume> for Command {
    fn from(c: ResultConsume) -> Self {
        Command::ResultConsume(c)
    }
}

#[must_use]
#[derive(Debug)]
pub(super) struct ResultConsumeResult {
    pub(super) result: TestKitResultT<SummaryWithQuery>,
}

#[derive(Debug, Clone)]
pub(super) struct SummaryWithQuery {
    pub(super) summary: Arc<Summary>,
    pub(super) query: Arc<String>,
    pub(super) parameters: Arc<Option<HashMap<String, ValueSend>>>,
}

impl From<ResultConsumeResult> for CommandResult {
    fn from(r: ResultConsumeResult) -> Self {
        CommandResult::ResultConsume(r)
    }
}

#[derive(Debug)]
pub(super) struct LastBookmarks {
    pub(super) session_id: BackendId,
}

impl From<LastBookmarks> for Command {
    fn from(c: LastBookmarks) -> Self {
        Command::LastBookmarks(c)
    }
}

impl LastBookmarks {
    fn real_response(&self, tx_res: &Sender<CommandResult>, session: &Session) {
        self.buffered_response(tx_res, session.last_bookmarks())
    }

    fn buffered_response(&self, tx_res: &Sender<CommandResult>, bookmarks: Arc<Bookmarks>) {
        let msg = LastBookmarksResult {
            result: Ok(bookmarks),
        };
        tx_res.send(msg.into()).unwrap();
    }
}

#[must_use]
#[derive(Debug)]
pub(super) struct LastBookmarksResult {
    pub(super) result: TestKitResultT<Arc<Bookmarks>>,
}

impl From<LastBookmarksResult> for CommandResult {
    fn from(r: LastBookmarksResult) -> Self {
        CommandResult::LastBookmarks(r)
    }
}

#[must_use]
#[derive(Debug)]
pub(super) struct CloseResult {
    pub(super) result: TestKitResult,
}

impl From<CloseResult> for CommandResult {
    fn from(r: CloseResult) -> Self {
        CommandResult::Close(r)
    }
}

pub(super) type RecordKeys = Vec<Arc<String>>;
