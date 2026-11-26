use crate::{formats::{self,HEADER},model::Instance};
use anyhow::{ensure,bail,Context,Result};
use calamine::{Data,Reader,Xlsx};
use rust_xlsxwriter::Workbook;
use std::io::Cursor;

fn cell(v:&Data,port:bool)->Result<String>{
    match v{
        Data::Empty=>Ok(String::new()),
        Data::String(s)=>Ok(s.clone()),
        Data::Int(n) if port=>Ok(n.to_string()),
        Data::Float(n) if port && n.is_finite() && n.fract()==0.0=>Ok(format!("{n:.0}")),
        _=>bail!("text cells required (Port may be an integer); use text format for IDs"),
    }
}
pub fn read(bytes:&[u8])->Result<Vec<Instance>>{
    ensure!(bytes.len()<=32*1024*1024,"XLSX file exceeds 32 MiB");
    // Reject oversized ZIP expansions before parsing. XLSX has no executable macros here.
    let mut archive=zip::ZipArchive::new(Cursor::new(bytes))?;
    ensure!(archive.len()<=2048,"too many XLSX ZIP entries");
    let mut total=0u64;
    for i in 0..archive.len(){let entry=archive.by_index(i)?;total=total.checked_add(entry.size()).context("XLSX size overflow")?;ensure!(total<=64*1024*1024,"XLSX expanded size exceeds 64 MiB");}
    drop(archive);
    let mut book:Xlsx<_>=Xlsx::new(Cursor::new(bytes))?;
    let names=book.sheet_names();
    ensure!(names.len()==1,"XLSX must contain exactly one worksheet");
    let name=&names[0];
    let formulas=book.worksheet_formula(name)?;
    ensure!(!formulas.rows().flatten().any(|f|!f.is_empty()),"formula cells are not supported");
    let range=book.worksheet_range(name)?;
    ensure!(range.height()<=10001 && range.width()<=6,"XLSX maximum 10000 rows and six columns");
    let mut lines=range.rows();
    let header=lines.next().context("empty worksheet")?;
    let keys=header.iter().map(|v|cell(v,false)).collect::<Result<Vec<_>>>()?;
    ensure!(keys.iter().map(String::as_str).eq(HEADER),"invalid XLSX header");
    let mut rows=Vec::new();
    for (i,line) in lines.enumerate(){
        if line.iter().all(|v|matches!(v,Data::Empty)){continue;}
        let fields=(0..6).map(|j|line.get(j).map(|v|cell(v,j==3)).unwrap_or_else(||Ok(String::new()))).collect::<Result<Vec<_>>>().with_context(||format!("row {}",i+2))?;
        rows.push(formats::from_fields(&fields.iter().map(String::as_str).collect::<Vec<_>>(),true).with_context(||format!("row {}",i+2))?);
    }
    formats::validate_rows(rows)
}
pub fn write(rows:&[Instance])->Result<Vec<u8>>{
    ensure!(rows.len()<=10000,"XLSX maximum 10000 rows; use JSON/CSV for larger exports");
    let mut book=Workbook::new();
    let sheet=book.add_worksheet();
    sheet.set_name("Instances")?;
    for (j,h) in HEADER.iter().enumerate(){sheet.write_string(0,j as u16,*h)?;}
    for (i,r) in rows.iter().enumerate(){
        let values=[r.name.clone(),r.id.clone(),r.address.clone(),r.port.to_string(),serde_json::to_string(&r.tags)?,serde_json::to_string(&r.meta)?];
        for (j,value) in values.iter().enumerate(){sheet.write_string((i+1) as u32,j as u16,value.as_str())?;}
    }
    for j in 0..6 {sheet.set_column_width(j,if j>=4{40.0}else{23.0})?;}
    Ok(book.save_to_buffer()?)
}
