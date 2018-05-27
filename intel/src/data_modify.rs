//! collection of functions that modify the database
//! using UPDATE and DELETE SQL

use common;
use data_container::RecordAction;
use data_container::RecordChangeset;
use data_container::SaveContainer;
use error::IntelError;
use rustorm;
use rustorm::ColumnName;
use rustorm::DbError;
use rustorm::Record;
use rustorm::RecordManager;
use rustorm::Rows;
use rustorm::Table;
use rustorm::TableName;
use rustorm::Value;
use tab;
use tab::Tab;
use table_intel;
use window::Window;

/// delete the records with the following record_ids
/// return the total number of records deleted
pub fn delete_records(
    dm: &RecordManager,
    main_table: &Table,
    record_ids: &Vec<String>,
) -> Result<Rows, IntelError> {
    let pk_types = &main_table.get_primary_column_types();
    let primary_columns = &main_table.get_primary_column_names();
    let mut record_id_values = Vec::with_capacity(record_ids.len());
    for rid in record_ids.iter() {
        let record_id_value: Vec<(&ColumnName, Value)> =
            common::extract_record_id(rid, pk_types, primary_columns)?;
        record_id_values.push(record_id_value);
    }
    if primary_columns.len() == 1 {
        let rows = delete_records_from_single_primary_column(dm, main_table, &record_id_values)?;
        Ok(rows)
    } else {
        panic!("not yet handled composite primary key")
    }
}

fn delete_records_from_single_primary_column(
    dm: &RecordManager,
    main_table: &Table,
    record_ids: &Vec<Vec<(&ColumnName, Value)>>,
) -> Result<Rows, DbError> {
    let table_name = &main_table.name;
    let primary_columns = &main_table.get_primary_column_names();
    assert_eq!(primary_columns.len(), 1);
    let pk_column = primary_columns[0];
    let mut sql = format!("DELETE FROM {} ", table_name.complete_name());
    sql += &format!("WHERE {} IN (", pk_column.name);
    let mut pk_values = Vec::with_capacity(record_ids.len());
    for (i, record_id) in record_ids.iter().enumerate() {
        assert_eq!(record_id.len(), 1);
        let pk_record_id = &record_id[0];
        let pk_value = pk_record_id.1.to_owned();
        if i > 0 {
            sql += ", ";
        }
        sql += &format!("${} ", i + 1);
        pk_values.push(pk_value);
    }
    sql += ") ";
    sql += "RETURNING *";
    let rows = dm.execute_sql_with_return(&sql, &pk_values)?;
    Ok(rows)
}

pub fn save_container(
    dm: &RecordManager,
    tables: &Vec<Table>,
    container: &SaveContainer,
) -> Result<(), IntelError> {
    println!("container: {:#?}", container);
    let &(ref table_name_for_insert, ref rows_insert) = &container.for_insert;
    let &(ref table_name_for_update, ref rows_update) = &container.for_update;
    println!("rows_update: {:?}", rows_update);
    let table_for_insert = table_intel::get_table(table_name_for_insert, tables).unwrap();
    let table_for_update = table_intel::get_table(table_name_for_update, tables).unwrap();
    if rows_insert.iter().count() > 0 {
        insert_rows_to_table(dm, table_for_insert, rows_insert)?;
    }
    update_records_in_table(dm, table_for_update, rows_update)?;
    Ok(())
}

pub fn save_changeset(
    dm: &RecordManager,
    tables: &Vec<Table>,
    window: &Window,
    table: &Table,
    changeset: &RecordChangeset,
) -> Result<(), IntelError> {
    println!("saving changeset: {:#?}", changeset);
    let updated_record = match &changeset.action {
        RecordAction::CreateNew => insert_record_to_table(dm, table, &changeset.record)?,
        RecordAction::Edited => update_record_in_table(dm, table, &changeset.record)?,
        _ => panic!("unhandled case: {:?}", changeset.action),
    };
    println!("updated record: {:?}", updated_record);
    save_one_ones(
        dm,
        tables,
        table,
        &updated_record,
        &window.one_one_tabs,
        &changeset.one_ones,
    )?;
    save_has_many(
        dm,
        tables,
        table,
        &updated_record,
        &window.has_many_tabs,
        &changeset.has_many,
    )?;
    save_indirect(
        dm,
        tables,
        table,
        &updated_record,
        &window.indirect_tabs,
        &changeset.indirect,
    )?;
    Ok(())
}

fn save_one_ones(
    dm: &RecordManager,
    tables: &Vec<Table>,
    main_table: &Table,
    main_record: &Record,
    _one_one_tabs: &Vec<Tab>,
    one_one_records: &Vec<(TableName, Option<Record>)>,
) -> Result<(), IntelError> {
    println!("saving one ones: {:?}", one_one_records);
    for (one_one_table_name, one_one_record) in one_one_records {
        if let Some(one_one_record) = one_one_record {
            //TODO: verify that the one_one_table_name belongs to the one_one_tabs
            if let Some(one_one_table) = table_intel::get_table(one_one_table_name, tables) {
                save_one_one_table(
                    dm,
                    tables,
                    main_table,
                    main_record,
                    one_one_table,
                    one_one_record,
                )?;
            }
        }
    }
    Ok(())
}

fn save_one_one_table(
    dm: &RecordManager,
    _tables: &Vec<Table>,
    main_table: &Table,
    main_record: &Record,
    one_one_table: &Table,
    one_one_record: &Record,
) -> Result<Record, DbError> {
    println!("save one_one_record: {:#?}", one_one_record);
    upsert_one_one_record_to_table(dm, main_table, main_record, one_one_table, one_one_record)
}

fn save_has_many(
    dm: &RecordManager,
    tables: &Vec<Table>,
    main_table: &Table,
    main_record: &Record,
    has_many_tabs: &Vec<Tab>,
    has_many_records: &Vec<(TableName, RecordAction, Rows)>,
) -> Result<(), IntelError> {
    println!("saving has_many : {:?}", has_many_records);
    for (has_many_table_name, record_action, has_many_rows) in has_many_records {
        let _has_many_tab = tab::find_tab(has_many_tabs, has_many_table_name)
            .expect("table should belong to the tabs");
        let has_many_table =
            table_intel::get_table(has_many_table_name, tables).expect("table should exist");
        save_has_many_table(
            dm,
            tables,
            main_table,
            main_record,
            has_many_table,
            record_action,
            &has_many_rows,
        )?;
    }
    Ok(())
}

fn save_has_many_table(
    dm: &RecordManager,
    _tables: &Vec<Table>,
    _main_table: &Table,
    _main_record: &Record,
    has_many_table: &Table,
    record_action: &RecordAction,
    has_many_rows: &Rows,
) -> Result<(), IntelError> {
    println!("record action: {:?}", record_action);
    match record_action {
        RecordAction::Unlink => {
            println!("UnLink.. deleting records");
            delete_from_table(dm, has_many_table, has_many_rows)?;
        }
        RecordAction::LinkNew => {
            println!("LinkNew... adding records");
            if has_many_rows.iter().count() > 0 {
                insert_rows_to_table(dm, has_many_table, has_many_rows)?;
            }
        }
        RecordAction::Edited => {
            println!("Editing in has many is ok?");
            update_records_in_table(dm, has_many_table, has_many_rows)?;
        }
        _ => panic!("unexpected record action: {:?} in has_many", record_action),
    }
    Ok(())
}

fn delete_from_table(dm: &RecordManager, table: &Table, rows: &Rows) -> Result<(), IntelError> {
    println!("delete_from_table : {:#?}", rows);
    for dao in rows.iter() {
        let record = Record::from(&dao);
        delete_record_from_table(dm, table, &record)?;
    }
    Ok(())
}

fn delete_record_from_table(
    dm: &RecordManager,
    table: &Table,
    record: &Record,
) -> Result<(), IntelError> {
    let mut params = vec![];
    let mut sql = String::from("DELETE FROM ");
    sql += &format!("{} ", table.complete_name());
    sql += "WHERE ";
    let pk = table.get_primary_column_names();
    for (i, col) in pk.iter().enumerate() {
        if i > 0 {
            sql += "AND ";
        }
        let pk_value = record
            .get_value(&col.name)
            .expect("must have primary column values");
        sql += &format!("{} = ${}", col.name, i + 1);
        params.push(pk_value);
    }
    println!("sql: {}", sql);
    println!("params: {:?}", params);
    dm.execute_sql_with_return(&sql, &params)?;
    Ok(())
}

fn save_indirect(
    dm: &RecordManager,
    tables: &Vec<Table>,
    main_table: &Table,
    main_record: &Record,
    _indirect_tabs: &Vec<(TableName, Tab)>,
    indirect_records: &Vec<(TableName, TableName, RecordAction, Rows)>,
) -> Result<(), IntelError> {
    println!("saving indirect: {:?}", indirect_records);
    for (indirect_tablename, via_tablename, record_action, rows) in indirect_records {
        let indirect_table = table_intel::get_table(indirect_tablename, tables)
            .expect("indirect table should exist");
        let linker_table =
            table_intel::get_table(via_tablename, tables).expect("via table should exists");
        match record_action {
            RecordAction::Unlink => {
                println!("deleting indirect_records");
                unlink_from_indirect_table(
                    dm,
                    tables,
                    main_table,
                    main_record,
                    indirect_table,
                    linker_table,
                    rows,
                )?;
            }
            RecordAction::LinkNew => {
                link_new_for_indirect_table(
                    dm,
                    tables,
                    main_table,
                    main_record,
                    indirect_table,
                    linker_table,
                    rows,
                )?;
            }
            RecordAction::LinkExisting => {
                link_existing_for_indirect_table(
                    dm,
                    tables,
                    main_table,
                    main_record,
                    indirect_table,
                    linker_table,
                    rows,
                )?;
            }
            _ => {
                println!("unexpected action {:?}", record_action);
            }
        }
    }
    Ok(())
}

/// delete the entry from the linker table
fn unlink_from_indirect_table(
    dm: &RecordManager,
    _tables: &Vec<Table>,
    main_table: &Table,
    main_record: &Record,
    indirect_table: &Table,
    linker_table: &Table,
    rows: &Rows,
) -> Result<(), IntelError> {
    println!("unlinking records in table");
    for dao in rows.iter() {
        let indirect_record = Record::from(&dao);
        let linker_record = create_linker_record(
            main_table,
            main_record,
            linker_table,
            indirect_table,
            &indirect_record,
        )?;
        delete_record_from_table(dm, linker_table, &linker_record)?;
    }
    Ok(())
}

/// create an entry to the indirect table
/// and create an entry into the linker table
fn link_new_for_indirect_table(
    dm: &RecordManager,
    _tables: &Vec<Table>,
    main_table: &Table,
    main_record: &Record,
    indirect_table: &Table,
    linker_table: &Table,
    rows: &Rows,
) -> Result<(), IntelError> {
    println!("link new for indirect table");
    for dao in rows.iter() {
        let indirect_record = Record::from(&dao);
        let indirect_record = insert_record_to_table(dm, indirect_table, &indirect_record)?;
        let linker_record = create_linker_record(
            main_table,
            main_record,
            linker_table,
            indirect_table,
            &indirect_record,
        )?;
        insert_record_to_linker_table(dm, linker_table, &linker_record)?;
    }
    Ok(())
}

/// create a record in linker table using the primary key of main and indirect record
fn create_linker_record(
    main_table: &Table,
    main_record: &Record,
    linker_table: &Table,
    indirect_table: &Table,
    indirect_record: &Record,
) -> Result<Record, IntelError> {
    println!("creating a record in linker table");
    let main_fk_pair = linker_table.get_local_foreign_columns_pair_to_table(&main_table.name);
    let indirect_fk_pair =
        linker_table.get_local_foreign_columns_pair_to_table(&indirect_table.name);
    println!("main_fk_pair: {:?}", main_fk_pair);
    println!("indirect_fk_pair: {:?}", indirect_fk_pair);
    assert_eq!(main_fk_pair.len(), 1);
    assert_eq!(indirect_fk_pair.len(), 1);
    let (main_linker_local, main_linker_refferred) = main_fk_pair[0];
    let (indirect_linker_local, indirect_linker_refferred) = indirect_fk_pair[0];
    let main_pk_value = main_record
        .get_value(&main_linker_refferred.name)
        .expect("must have a value");
    let indirect_pk_value = indirect_record
        .get_value(&indirect_linker_refferred.name)
        .expect("must have a value");
    let mut linker_record = Record::new();
    linker_record.insert_value(&main_linker_local.name, main_pk_value);
    linker_record.insert_value(&indirect_linker_local.name, indirect_pk_value);
    println!("linker table: {:#?}", linker_table);
    println!("linker_record: {:#?}", linker_record);
    Ok(linker_record)
}

/// create an entry to the linker table
/// linking existing record from the indirect table
fn link_existing_for_indirect_table(
    dm: &RecordManager,
    _tables: &Vec<Table>,
    main_table: &Table,
    main_record: &Record,
    indirect_table: &Table,
    linker_table: &Table,
    rows: &Rows,
) -> Result<(), IntelError> {
    println!("link existing for indirect table");
    println!("HERE.... <------");
    println!("rows: {:#?}", rows);
    for dao in rows.iter() {
        println!("dao: {:?}", dao);
        let indirect_record = Record::from(&dao);
        println!("indirect existing record: {:?}", indirect_record);
        let linker_record = create_linker_record(
            main_table,
            main_record,
            linker_table,
            indirect_table,
            &indirect_record,
        )?;
        insert_record_to_linker_table(dm, linker_table, &linker_record)?;
    }
    Ok(())
}

/// triggered by the main tab
fn update_records_in_table(
    dm: &RecordManager,
    main_table: &Table,
    rows: &Rows,
) -> Result<Vec<Record>, IntelError> {
    let mut records = vec![];
    for dao in rows.iter() {
        println!("dao: {:?}", dao);
        let record = Record::from(&dao);
        println!("record: {:?}", record);
        let updated_record = update_record_in_table(dm, main_table, &record)?;
        println!("updated record: {:?}", updated_record);
        records.push(updated_record);
    }
    Ok(records)
}

fn update_record_in_table(
    dm: &RecordManager,
    main_table: &Table,
    record: &Record,
) -> Result<Record, DbError> {
    let table_name = &main_table.name;
    let mut params = vec![];
    let mut sql = format!("UPDATE {} ", table_name.complete_name());
    let columns = main_table.get_non_primary_columns();
    sql += "SET ";
    for (i, col) in columns.iter().enumerate() {
        if i > 0 {
            sql += ", ";
        }
        sql += &format!("{} = ${}", col.name.name, i + 1);
        let value = record.get_value(&col.name.name);
        assert!(value.is_some());
        let value = value.unwrap();
        let casted_value = rustorm::common::cast_type(&value, &col.get_sql_type());
        params.push(casted_value);
    }
    sql += " ";
    let non_pk_columns_len = columns.len();
    let primary_columns = &main_table.get_primary_columns();
    for (i, pk) in primary_columns.iter().enumerate() {
        if i == 0 {
            sql += "WHERE ";
        } else {
            sql += "AND ";
        }
        sql += &format!("{} = ${} ", pk.name.name, non_pk_columns_len + i + 1);
        let pk_value = record.get_value(&pk.name.name);
        assert!(pk_value.is_some());
        let pk_value = pk_value.unwrap();
        let casted_pk_value = rustorm::common::cast_type(&pk_value, &pk.get_sql_type());
        params.push(casted_pk_value);
    }
    sql += "RETURNING *";

    println!("sql: {}", sql);
    println!("params: {:?}", params);
    dm.execute_sql_with_one_return(&sql, &params)
}

/// insert rows all at once in one query
fn insert_rows_to_table(
    dm: &RecordManager,
    table: &Table,
    rows: &Rows,
) -> Result<Rows, IntelError> {
    let table_name = &table.name;
    let mut params = vec![];
    let mut sql = format!("INSERT INTO {} ", table_name.complete_name());
    let columns = &table.get_non_primary_columns();
    sql += "(";
    for (i, col) in columns.iter().enumerate() {
        if are_all_nil(&col.name.name, rows) && col.is_not_null() && col.has_generated_default() {
            println!("skipping column: {}", col.name.name);
        } else {
            if i > 0 {
                sql += ", ";
            }
            sql += &format!("{} ", col.name.name);
        }
    }
    sql += ") ";
    sql += "VALUES (";
    for dao in rows.iter() {
        for (i, col) in columns.iter().enumerate() {
            let value = dao.get_value(&col.name.name);
            assert!(value.is_some());
            let value = value.unwrap();
            if value == &Value::Nil && col.is_not_null() && col.has_generated_default() {
                println!("skipping column: {}", col.name.name);
            } else {
                if i > 0 {
                    sql += ", ";
                }
                sql += &format!("${} ", params.len() + 1);
                let casted_value = rustorm::common::cast_type(&value, &col.get_sql_type());
                params.push(casted_value);
            }
        }
    }
    sql += ") RETURNING *";
    println!("sql: {}", sql);
    println!("params: {:?}", params);
    let rows = dm.execute_sql_with_return(&sql, &*params)?;
    Ok(rows)
}

/// check if all values in these columns are nill,
/// so we can skip it when column is skippable
fn are_all_nil(column: &str, rows: &Rows) -> bool {
    for dao in rows.iter() {
        if let Some(Value::Nil) = dao.get_value(column) {
            ;
        } else {
            return false;
        }
    }
    true
}

/// insert rows 1 by 1
fn insert_records_to_table1(
    dm: &RecordManager,
    main_table: &Table,
    rows: &Rows,
) -> Result<Vec<Record>, IntelError> {
    let mut records = vec![];
    for dao in rows.iter() {
        let record = Record::from(&dao);
        let updated_record = insert_record_to_table(dm, main_table, &record)?;
        records.push(updated_record);
    }
    Ok(records)
}

fn insert_record_to_table(
    dm: &RecordManager,
    main_table: &Table,
    record: &Record,
) -> Result<Record, DbError> {
    let table_name = &main_table.name;
    let mut params = vec![];
    let mut sql = format!("INSERT INTO {} ", table_name.complete_name());
    let columns = &main_table.get_non_primary_columns();
    sql += "(";
    for (i, col) in columns.iter().enumerate() {
        let value = record.get_value(&col.name.name);
        if let Some(value) = value {
            if value == Value::Nil && col.is_not_null() && col.has_generated_default() {
                println!("skipping column: {}", col.name.name);
            } else {
                if i > 0 {
                    sql += ", ";
                }
                sql += &format!("{} ", col.name.name);
            }
        } else {
            println!("skipping {} ", col.name.name);
        }
    }
    sql += ") ";
    sql += "VALUES (";
    for (i, col) in columns.iter().enumerate() {
        let value = record.get_value(&col.name.name);
        if let Some(value) = value {
            if value == Value::Nil && col.is_not_null() && col.has_generated_default() {
                println!("skipping column: {}", col.name.name);
            } else {
                if i > 0 {
                    sql += ", ";
                }
                sql += &format!("${} ", params.len() + 1);
                let casted_value = rustorm::common::cast_type(&value, &col.get_sql_type());
                params.push(casted_value);
            }
        } else {
            println!("skipping {} ", col.name.name);
        }
    }
    sql += ") RETURNING *";
    println!("sql: {}", sql);
    println!("params: {:?}", params);
    dm.execute_sql_with_one_return(&sql, &params)
}

fn insert_record_to_linker_table(
    dm: &RecordManager,
    linker_table: &Table,
    record: &Record,
) -> Result<Record, DbError> {
    let table_name = &linker_table.name;
    let mut params = vec![];
    let mut sql = format!("INSERT INTO {} ", table_name.complete_name());
    let columns = &linker_table.get_primary_columns();
    sql += "(";
    for (i, col) in columns.iter().enumerate() {
        let value = record.get_value(&col.name.name);
        if let Some(value) = value {
            if value == Value::Nil && col.is_not_null() && col.has_generated_default() {
                println!("skipping column: {}", col.name.name);
            } else {
                if i > 0 {
                    sql += ", ";
                }
                sql += &format!("{} ", col.name.name);
            }
        } else {
            println!("skipping {} ", col.name.name);
        }
    }
    sql += ") ";
    sql += "VALUES (";
    for (i, col) in columns.iter().enumerate() {
        let value = record.get_value(&col.name.name);
        if let Some(value) = value {
            if value == Value::Nil && col.is_not_null() && col.has_generated_default() {
                println!("skipping column: {}", col.name.name);
            } else {
                if i > 0 {
                    sql += ", ";
                }
                sql += &format!("${} ", params.len() + 1);
                let casted_value = rustorm::common::cast_type(&value, &col.get_sql_type());
                params.push(casted_value);
            }
        } else {
            println!("skipping {} ", col.name.name);
        }
    }
    sql += ") RETURNING *";
    println!("sql: {}", sql);
    println!("params: {:?}", params);
    dm.execute_sql_with_one_return(&sql, &params)
}

/// Warning: This only works for postgresql 9.5 and up
/// TODO: make the database trait tell which version is in used
/// use appropriate query for depending on which features are supported
fn upsert_one_one_record_to_table(
    dm: &RecordManager,
    main_table: &Table,
    main_record: &Record,
    one_one_table: &Table,
    one_one_record: &Record,
) -> Result<Record, DbError> {
    let local_referred_pair =
        one_one_table.get_local_foreign_columns_pair_to_table(&main_table.name);

    let mut one_one_record = one_one_record.clone();

    for (one_one_pk, main_pk_name) in local_referred_pair.iter() {
        let main_pk_value = main_record
            .get_value(&main_pk_name.name)
            .expect("should have value");
        one_one_record.insert_value(&one_one_pk.name, main_pk_value);
    }

    let one_one_table_name = &one_one_table.name;
    let mut params = vec![];
    let mut sql = format!("INSERT INTO {} ", one_one_table_name.complete_name());
    let one_one_columns = &one_one_table.columns;
    sql += "(";

    for (i, one_col) in one_one_columns.iter().enumerate() {
        let value = one_one_record.get_value(&one_col.name.name);
        assert!(value.is_some());
        let value = value.unwrap();
        if value == Value::Nil && one_col.is_not_null() && one_col.has_generated_default() {
            println!("skipping column: {}", one_col.name.name);
        } else {
            if i > 0 {
                sql += ", ";
            }
            sql += &format!("{} ", one_col.name.name);
        }
    }
    sql += ") ";
    sql += "VALUES (";

    for (i, one_col) in one_one_columns.iter().enumerate() {
        let value = one_one_record.get_value(&one_col.name.name);
        assert!(value.is_some());
        let value = value.unwrap();
        if value == Value::Nil && one_col.is_not_null() && one_col.has_generated_default() {
            println!("skipping column: {}", one_col.name.name);
        } else {
            if i > 0 {
                sql += ", ";
            }
            sql += &format!("${} ", params.len() + 1);
            let casted_value = rustorm::common::cast_type(&value, &one_col.get_sql_type());
            params.push(casted_value);
        }
    }
    sql += ") ";
    let one_one_primary_columns = &one_one_table.get_primary_columns();
    sql += "ON CONFLICT (";

    for (i, one_one_pk) in one_one_primary_columns.iter().enumerate() {
        if i == 0 {
            ;
        } else {
            sql += ", ";
        }
        sql += &format!("{} ", one_one_pk.name.name);
    }
    sql += ") ";
    sql += "DO UPDATE ";
    sql += "SET ";

    for (i, one_col) in one_one_columns.iter().enumerate() {
        if i > 0 {
            sql += ", ";
        }
        sql += &format!("{} = ${}", one_col.name.name, params.len() + 1);
        let value = one_one_record
            .get_value(&one_col.name.name)
            .expect("must have value");
        let casted_value = rustorm::common::cast_type(&value, &one_col.get_sql_type());
        params.push(casted_value);
    }
    sql += " ";

    for (i, (one_one_pk, main_pk_name)) in local_referred_pair.iter().enumerate() {
        if i == 0 {
            sql += "WHERE ";
        } else {
            sql += "AND ";
        }
        sql += &format!(
            "{}.{} = ${} ",
            one_one_table.name.name,
            one_one_pk.name,
            params.len() + 1
        );
        let main_pk = main_table.get_column(main_pk_name).expect("should exist");
        let pk_value = main_record.get_value(&main_pk.name.name);
        assert!(pk_value.is_some());
        let pk_value = pk_value.unwrap();
        let casted_pk_value = rustorm::common::cast_type(&pk_value, &main_pk.get_sql_type());
        params.push(casted_pk_value);
    }
    sql += "RETURNING *";
    println!("sql: {}", sql);
    println!("params: {:?}", params);
    dm.execute_sql_with_one_return(&sql, &params)
}
