//! Golden table for the formula engine: one row per
//! `(formula, expected canonical value)`.
//!
//! # Why a table
//!
//! `opendoc-spreadsheet` shipped a 258-function engine with no test in
//! `functions.rs`, `functions_legacy.rs`, `functions_legacy_tail.rs`,
//! `formula.rs`, `value.rs`, `format.rs`, `lookup.rs` or `address.rs`. Six
//! wrong answers reached users through that gap — `COUNT` over a range
//! holding an error, `MAX`/`MIN`/`PRODUCT` over an empty range,
//! one-argument `ROUND`, `FLOOR` with a negative number, `SUBTOTAL`
//! double-counting nested subtotals, and every number past 2^63 printing as
//! the same saturated integer. Each is one line in the table below, and each
//! would have been caught the day it was written.
//!
//! # How to read a row
//!
//! The left column is the formula as typed into a cell. The right column is
//! the cell's canonical computed text: digits for a number, the text for a
//! string, `true`/`false` for a boolean, the error code (`#DIV/0!`,
//! `#VALUE!`, `#NAME?`, `#N/A`, `#NUM!`, `#REF!`) for an error, and the
//! empty string for a blank. That is `FormulaValue::canonical_text`, the
//! same text the grid stores in `Cell::computed_value`.
//!
//! A row is not a record of what the engine does — it is the answer every
//! other spreadsheet gives. Adding a row means deciding the right answer
//! first and then making the engine produce it.
//!
//! # The fixture
//!
//! Every formula is evaluated in cell `Y1` of one sheet, against
//! [`FIXTURE`]. The grid is deliberately shaped around the cases that go
//! wrong:
//!
//! - `A1:A5` = 1..5 and `B1:B5` = 10..50 — a clean numeric pair for
//!   aggregates, lookup and regression.
//! - `C1` is `=1/0` — an **error inside a range**, so `A1:D1` mixes two
//!   numbers, an error and text.
//! - `D1:D4` are words with a repeat, for `COUNTIF`, `UNIQUE` and wildcards.
//! - `E1:E3` are booleans, which ranges skip and direct arguments coerce.
//! - `G1:G6` is a column of values with `=SUBTOTAL(...)` group totals inside
//!   it, so a `SUBTOTAL` spanning the column must not count them twice.
//! - `H1:H4` are *typed* inputs — `2024-01-05`, `50%`, `$5`, `1,000` — which
//!   are parsed on the way in and must behave as the numbers they name.
//! - `Z1:Z9` is deliberately **empty**, for empty-range behaviour.

use crate::SpreadsheetWorkbook;

/// The grid every row of [`GOLDEN`] is evaluated against.
const FIXTURE: &[(&str, &str)] = &[
    ("A1", "1"),
    ("A2", "2"),
    ("A3", "3"),
    ("A4", "4"),
    ("A5", "5"),
    ("B1", "10"),
    ("B2", "20"),
    ("B3", "30"),
    ("B4", "40"),
    ("B5", "50"),
    ("C1", "=1/0"),
    ("C2", "2"),
    ("C3", "x"),
    ("C4", "-4"),
    ("C5", "2.5"),
    ("D1", "apple"),
    ("D2", "banana"),
    ("D3", "cherry"),
    ("D4", "apple"),
    ("E1", "TRUE"),
    ("E2", "FALSE"),
    ("E3", "TRUE"),
    ("G1", "1"),
    ("G2", "2"),
    ("G3", "=SUBTOTAL(9,G1:G2)"),
    ("G4", "4"),
    ("G5", "5"),
    ("G6", "=SUBTOTAL(9,G4:G5)"),
    ("H1", "2024-01-05"),
    ("H2", "50%"),
    ("H3", "$5"),
    ("H4", "1,000"),
];

/// `(formula, expected canonical value)`.
const GOLDEN: &[(&str, &str)] = &[
    // Arithmetic, rounding and trigonometry
    ("=ABS(-3)", "3"),
    ("=ABS(3)", "3"),
    ("=SIGN(-7)", "-1"),
    ("=SIGN(0)", "0"),
    ("=SQRT(9)", "3"),
    ("=SQRT(-1)", "#NUM!"),
    ("=EXP(0)", "1"),
    ("=EXP(1)", "2.718281828459045"),
    ("=LN(1)", "0"),
    ("=LN(0)", "#NUM!"),
    ("=LN(-1)", "#NUM!"),
    ("=LOG(100)", "2"),
    ("=LOG(8,2)", "3"),
    ("=LOG10(1000)", "3"),
    ("=POWER(2,10)", "1024"),
    ("=POWER(-8,1/3)", "#NUM!"),
    ("=2^10", "1024"),
    ("=MOD(10,3)", "1"),
    ("=MOD(-10,3)", "2"),
    ("=MOD(10,0)", "#DIV/0!"),
    ("=QUOTIENT(10,3)", "3"),
    ("=QUOTIENT(-10,3)", "-3"),
    ("=INT(2.7)", "2"),
    ("=INT(-2.7)", "-3"),
    ("=EVEN(1.5)", "2"),
    ("=EVEN(-1.5)", "-2"),
    ("=ODD(1.5)", "3"),
    ("=ODD(-1.5)", "-3"),
    ("=TRUNC(2.789)", "2"),
    ("=TRUNC(2.789,2)", "2.78"),
    ("=TRUNC(-2.789)", "-2"),
    ("=ROUND(2.5)", "3"),
    ("=ROUND(-2.5)", "-3"),
    ("=ROUND(2.4)", "2"),
    ("=ROUND(2.345,2)", "2.35"),
    ("=ROUND(1234,-2)", "1200"),
    ("=ROUNDUP(2.1)", "3"),
    ("=ROUNDDOWN(2.9)", "2"),
    ("=ROUNDUP(-2.1)", "-3"),
    ("=ROUNDDOWN(-2.9)", "-2"),
    ("=MROUND(10,3)", "9"),
    ("=MROUND(-10,3)", "#NUM!"),
    ("=MROUND(10,0)", "0"),
    ("=CEILING(-4.5)", "-4"),
    ("=CEILING(4.5,2)", "6"),
    ("=CEILING(-4.5,2)", "-4"),
    ("=CEILING(-4.5,-2)", "-6"),
    ("=CEILING(4.5,-2)", "#NUM!"),
    ("=CEILING(4.5,0)", "#DIV/0!"),
    ("=FLOOR(-4.5)", "-5"),
    ("=FLOOR(4.5)", "4"),
    ("=FLOOR(4.5,2)", "4"),
    ("=FLOOR(-4.5,2)", "-6"),
    ("=FLOOR(-4.5,-2)", "-6"),
    ("=FLOOR(4.5,-2)", "#NUM!"),
    ("=FLOOR(4.5,0)", "#DIV/0!"),
    ("=FLOOR.MATH(-4.5)", "-5"),
    ("=FLOOR.MATH(-4.5,2)", "-6"),
    ("=FLOOR.PRECISE(-4.5,2)", "-6"),
    ("=CEILING.MATH(-4.5)", "-4"),
    ("=CEILING.PRECISE(-4.5,2)", "-4"),
    ("=ISO.CEILING(-4.5,2)", "-4"),
    ("=FACT(5)", "120"),
    ("=FACT(0)", "1"),
    ("=FACT(-1)", "#NUM!"),
    ("=FACT(25)", "1.5511210043330986E+25"),
    ("=FACTDOUBLE(7)", "105"),
    ("=COMBIN(5,2)", "10"),
    ("=COMBINA(5,2)", "15"),
    ("=PERMUT(5,2)", "20"),
    ("=PERMUTATIONA(5,2)", "25"),
    ("=GCD(12,18)", "6"),
    ("=LCM(4,6)", "12"),
    ("=PI()", "3.141592653589793"),
    ("=DEGREES(PI())", "180"),
    ("=RADIANS(180)", "3.141592653589793"),
    ("=SIN(0)", "0"),
    ("=COS(0)", "1"),
    ("=TAN(0)", "0"),
    ("=ASIN(1)", "1.5707963267948966"),
    ("=ACOS(1)", "0"),
    ("=ATAN(1)", "0.7853981633974483"),
    ("=ATAN2(1,1)", "0.7853981633974483"),
    ("=ASIN(2)", "#NUM!"),
    ("=ACOS(2)", "#NUM!"),
    ("=SINH(0)", "0"),
    ("=COSH(0)", "1"),
    ("=TANH(0)", "0"),
    ("=ASINH(0)", "0"),
    ("=ACOSH(1)", "0"),
    ("=ACOSH(0)", "#NUM!"),
    ("=ATANH(0)", "0"),
    ("=ATANH(1)", "#NUM!"),
    ("=COT(1)", "0.6420926159343306"),
    ("=COT(0)", "#DIV/0!"),
    ("=COTH(1)", "1.3130352854993315"),
    ("=ACOT(1)", "0.7853981633974483"),
    ("=ACOTH(2)", "0.5493061443340549"),
    ("=SEC(0)", "1"),
    ("=SECH(0)", "1"),
    ("=CSC(1)", "1.1883951057781212"),
    ("=CSC(0)", "#DIV/0!"),
    ("=CSCH(1)", "0.8509181282393216"),
    ("=1E21", "1E+21"),
    ("=2^63", "9.223372036854776E+18"),
    ("=1/0", "#DIV/0!"),
    ("=0/0", "#DIV/0!"),
    // Text, dates and times
    ("=LEN(\"hello\")", "5"),
    ("=LEN(\"\")", "0"),
    ("=LEFT(\"hello\",2)", "he"),
    ("=LEFT(\"hello\")", "h"),
    ("=RIGHT(\"hello\",2)", "lo"),
    ("=MID(\"hello\",2,3)", "ell"),
    ("=MID(\"hello\",0,2)", "#VALUE!"),
    ("=UPPER(\"aBc\")", "ABC"),
    ("=LOWER(\"aBc\")", "abc"),
    ("=PROPER(\"hello world\")", "Hello World"),
    ("=TRIM(\"  a  b  \")", "a b"),
    ("=CONCAT(\"a\",\"b\")", "ab"),
    ("=CONCATENATE(\"a\",1,TRUE)", "a1TRUE"),
    ("=JOIN(\"-\",A1:A3)", "1-2-3"),
    ("=SPLIT(\"a,b,c\",\",\")", "a"),
    ("=FIND(\"l\",\"hello\")", "3"),
    ("=FIND(\"z\",\"hello\")", "#VALUE!"),
    ("=FIND(\"L\",\"hello\")", "#VALUE!"),
    ("=REPLACE(\"hello\",2,3,\"XY\")", "hXYo"),
    ("=REPT(\"ab\",3)", "ababab"),
    ("=REPT(\"ab\",0)", ""),
    ("=SUBSTITUTE(\"aaa\",\"a\",\"b\",2)", "aba"),
    ("=SUBSTITUTE(\"aaa\",\"a\",\"b\")", "bbb"),
    ("=EXACT(\"a\",\"A\")", "false"),
    ("=EXACT(\"a\",\"a\")", "true"),
    ("=CHAR(65)", "A"),
    ("=CHAR(0)", "#VALUE!"),
    ("=CODE(\"A\")", "65"),
    ("=CODE(\"\")", "#VALUE!"),
    ("=T(\"abc\")", "abc"),
    ("=T(1)", ""),
    ("=N(\"abc\")", "0"),
    ("=N(1)", "1"),
    ("=N(TRUE)", "1"),
    ("=VALUE(\"1,000\")", "1000"),
    ("=VALUE(\"abc\")", "#VALUE!"),
    ("=NUMBERVALUE(\"1.5\")", "1.5"),
    ("=TEXT(1234.5,\"#,##0.00\")", "1,234.50"),
    ("=TEXT(0.5,\"0%\")", "50%"),
    ("=TO_TEXT(1)", "1"),
    ("=DOLLAR(1234.5)", "$1,234.50"),
    ("=DOLLAR(1234.5,0)", "$1,235"),
    ("=FIXED(1234.5,1)", "1,234.5"),
    ("=FIXED(1234.5,1,TRUE)", "1234.5"),
    ("=REGEXMATCH(\"abc\",\"b\")", "true"),
    ("=REGEXEXTRACT(\"abc123\",\"[0-9]+\")", "123"),
    ("=REGEXEXTRACT(\"abc\",\"[0-9]+\")", "#N/A"),
    ("=LEN(A1)", "1"),
    ("=LEN(C1)", "#DIV/0!"),
    ("=UPPER(C1)", "#DIV/0!"),
    ("=DATE(2024,1,5)", "45296"),
    ("=YEAR(DATE(2024,1,5))", "2024"),
    ("=MONTH(DATE(2024,1,5))", "1"),
    ("=DAY(DATE(2024,1,5))", "5"),
    ("=DATE(2024,13,1)", "45658"),
    ("=DATE(2024,2,30)", "45352"),
    ("=TIME(12,30,0)", "0.5208333333333334"),
    ("=HOUR(TIME(12,30,0))", "12"),
    ("=MINUTE(TIME(12,30,0))", "30"),
    ("=SECOND(TIME(12,30,15))", "15"),
    ("=DAYS(DATE(2024,1,10),DATE(2024,1,1))", "9"),
    ("=DAYS360(DATE(2024,1,1),DATE(2024,3,1))", "60"),
    ("=NETWORKDAYS(DATE(2024,1,1),DATE(2024,1,7))", "5"),
    ("=WORKDAY(DATE(2024,1,1),5)", "45299"),
    ("=WEEKDAY(DATE(2024,1,7))", "1"),
    ("=WEEKDAY(DATE(2024,1,7),2)", "7"),
    ("=WEEKNUM(DATE(2024,1,7))", "2"),
    ("=ISOWEEKNUM(DATE(2024,1,7))", "1"),
    ("=EDATE(DATE(2024,1,31),1)", "45351"),
    ("=EOMONTH(DATE(2024,1,15),0)", "45322"),
    ("=DATEVALUE(\"2024-01-05\")", "45296"),
    ("=TIMEVALUE(\"12:30:00\")", "0.5208333333333334"),
    ("=DATEDIF(DATE(2024,1,1),DATE(2025,3,10),\"Y\")", "1"),
    ("=DATEDIF(DATE(2024,1,1),DATE(2025,3,10),\"M\")", "14"),
    ("=DATEDIF(DATE(2024,1,1),DATE(2025,3,10),\"D\")", "434"),
    ("=ISDATE(DATE(2024,1,5))", "true"),
    ("=ISDATE(\"abc\")", "false"),
    // Aggregates, statistics, regression and engineering
    ("=SUM(A1:A5)", "15"),
    ("=SUM(Z1:Z9)", "0"),
    ("=SUM(A1:D1)", "#DIV/0!"),
    ("=SUM(C1:C2)", "#DIV/0!"),
    ("=AVERAGE(A1:A5)", "3"),
    ("=AVERAGE(Z1:Z9)", "#DIV/0!"),
    ("=COUNT(A1:A5)", "5"),
    ("=COUNT(A1:D1)", "2"),
    ("=COUNT(D1:D4)", "0"),
    ("=COUNT(Z1:Z9)", "0"),
    ("=COUNT(1/0)", "#DIV/0!"),
    ("=COUNTA(A1:D1)", "4"),
    ("=COUNTA(Z1:Z9)", "0"),
    ("=COUNTBLANK(A1:D1)", "0"),
    ("=COUNTUNIQUE(D1:D4)", "3"),
    ("=COUNTIF(D1:D4,\"apple\")", "2"),
    ("=COUNTIF(D1:D4,\"a*\")", "2"),
    ("=COUNTIF(A1:A5,\">3\")", "2"),
    ("=COUNTIFS(A1:A5,\">2\",B1:B5,\"<50\")", "2"),
    ("=MAX(A1:A5)", "5"),
    ("=MAX(Z1:Z9)", "0"),
    ("=MAX(D1:D4)", "0"),
    ("=MIN(A1:A5)", "1"),
    ("=MIN(Z1:Z9)", "0"),
    ("=MINA(A1:A5)", "1"),
    ("=AVERAGEA(A1:A5)", "3"),
    ("=PRODUCT(A1:A3)", "6"),
    ("=PRODUCT(Z1:Z9)", "0"),
    ("=MEDIAN(A1:A5)", "3"),
    ("=MEDIAN(Z1:Z9)", "#NUM!"),
    ("=MODE(1,2,2,3)", "2"),
    ("=MODE(1,2,3)", "#N/A"),
    ("=SUMSQ(1,2,3)", "14"),
    ("=STDEV(A1:A5)", "1.5811388300841898"),
    ("=STDEVP(A1:A5)", "1.4142135623730951"),
    ("=VAR(A1:A5)", "2.5"),
    ("=VARP(A1:A5)", "2"),
    ("=AVEDEV(A1:A5)", "1.2"),
    ("=DEVSQ(A1:A5)", "10"),
    ("=GEOMEAN(A1:A5)", "2.6051710846973517"),
    ("=HARMEAN(A1:A5)", "2.18978102189781"),
    ("=LARGE(A1:A5,2)", "4"),
    ("=SMALL(A1:A5,2)", "2"),
    ("=LARGE(A1:A5,9)", "#NUM!"),
    ("=PERCENTILE(A1:A5,0.5)", "3"),
    ("=QUARTILE(A1:A5,1)", "2"),
    ("=PERCENTILE.EXC(A1:A5,0.5)", "3"),
    ("=QUARTILE.EXC(A1:A5,1)", "1.5"),
    ("=RANK(3,A1:A5)", "3"),
    ("=PERCENTRANK(A1:A5,3)", "0.5"),
    ("=STANDARDIZE(3,3,1)", "0"),
    ("=CORREL(A1:A5,B1:B5)", "1"),
    ("=COVAR(A1:A5,B1:B5)", "20"),
    ("=COVARIANCE.S(A1:A5,B1:B5)", "25"),
    ("=SLOPE(B1:B5,A1:A5)", "10"),
    ("=INTERCEPT(B1:B5,A1:A5)", "0"),
    ("=RSQ(B1:B5,A1:A5)", "1"),
    ("=FORECAST(6,B1:B5,A1:A5)", "60"),
    ("=FISHER(0.5)", "0.5493061443340549"),
    ("=FISHERINV(0.5)", "0.46211715726000974"),
    ("=ERF(1)", "0.8427006897475899"),
    ("=ERFC(1)", "0.15729931025241006"),
    ("=DELTA(1,1)", "1"),
    ("=DELTA(1,2)", "0"),
    ("=GESTEP(5,1)", "1"),
    ("=GESTEP(0,1)", "0"),
    ("=BASE(255,16)", "FF"),
    ("=BASE(255,16,4)", "00FF"),
    ("=DECIMAL(\"FF\",16)", "255"),
    ("=DECIMAL(\"ZZ\",16)", "#VALUE!"),
    ("=SUMPRODUCT(A1:A3,B1:B3)", "140"),
    ("=SUBTOTAL(9,G1:G6)", "12"),
    ("=SUBTOTAL(1,A1:A5)", "3"),
    ("=SUBTOTAL(2,A1:D1)", "2"),
    ("=SUBTOTAL(3,A1:D1)", "4"),
    ("=SUBTOTAL(109,A1:A5)", "15"),
    ("=SUBTOTAL(12,A1:A5)", "#VALUE!"),
    // Logical, information, lookup and array
    ("=IF(TRUE,1,2)", "1"),
    ("=IF(FALSE,1,2)", "2"),
    ("=IF(1,\"y\",\"n\")", "y"),
    ("=IF(\"x\",1,2)", "#VALUE!"),
    ("=IFS(FALSE,1,TRUE,2)", "2"),
    ("=IFS(FALSE,1)", "#N/A"),
    ("=SWITCH(2,1,\"a\",2,\"b\",\"z\")", "b"),
    ("=SWITCH(9,1,\"a\",2,\"b\")", "#N/A"),
    ("=IFERROR(1/0,\"caught\")", "caught"),
    ("=IFERROR(1,\"caught\")", "1"),
    ("=AND(TRUE,TRUE)", "true"),
    ("=AND(TRUE,FALSE)", "false"),
    ("=OR(FALSE,TRUE)", "true"),
    ("=XOR(TRUE,TRUE)", "false"),
    ("=NOT(TRUE)", "false"),
    ("=TRUE()", "true"),
    ("=FALSE()", "false"),
    ("=NA()", "#N/A"),
    ("=ERROR.TYPE(1/0)", "2"),
    ("=ERROR.TYPE(NA())", "7"),
    ("=ERROR.TYPE(1)", "#N/A"),
    ("=ISBLANK(Z1)", "true"),
    ("=ISBLANK(A1)", "false"),
    ("=ISERROR(1/0)", "true"),
    ("=ISERROR(1)", "false"),
    ("=ISERR(1/0)", "true"),
    ("=ISERR(NA())", "false"),
    ("=ISNA(NA())", "true"),
    ("=ISNA(1/0)", "false"),
    ("=ISNUMBER(1)", "true"),
    ("=ISNUMBER(\"a\")", "false"),
    ("=ISTEXT(\"a\")", "true"),
    ("=ISNONTEXT(1)", "true"),
    ("=ISLOGICAL(TRUE)", "true"),
    ("=ISREF(A1)", "true"),
    ("=ISEVEN(2)", "true"),
    ("=ISODD(3)", "true"),
    ("=ISBETWEEN(2,1,3)", "true"),
    ("=ISEMAIL(\"a@b.com\")", "true"),
    ("=ISEMAIL(\"nope\")", "false"),
    ("=ISURL(\"https://a.b\")", "false"),
    ("=TYPE(1)", "1"),
    ("=TYPE(\"a\")", "2"),
    ("=TYPE(TRUE)", "4"),
    ("=TYPE(1/0)", "16"),
    ("=VLOOKUP(3,A1:B5,2,FALSE)", "30"),
    ("=VLOOKUP(9,A1:B5,2,FALSE)", "#N/A"),
    ("=VLOOKUP(3,A1:B5,2)", "30"),
    ("=VLOOKUP(3,A1:B5,5,FALSE)", "#REF!"),
    ("=VLOOKUP(\"a*\",D1:D4,1,FALSE)", "apple"),
    ("=MATCH(3,A1:A5,0)", "3"),
    ("=MATCH(3,A1:A5)", "3"),
    ("=MATCH(9,A1:A5,0)", "#N/A"),
    ("=MATCH(\"a*\",D1:D4,0)", "1"),
    ("=MATCH(\"apple\",D1:D4,0)", "1"),
    ("=MATCH(\"APPLE\",D1:D4,0)", "1"),
    ("=INDEX(A1:B5,2,2)", "20"),
    ("=INDEX(A1:B5,9,1)", "#REF!"),
    ("=LOOKUP(3,A1:A5,B1:B5)", "30"),
    ("=XLOOKUP(3,A1:A5,B1:B5)", "30"),
    ("=XLOOKUP(9,A1:A5,B1:B5,\"none\")", "none"),
    ("=XLOOKUP(\"a*\",D1:D4,A1:A4,\"none\",2)", "1"),
    ("=ROW(A3)", "3"),
    ("=COLUMN(C1)", "3"),
    ("=ROWS(A1:A5)", "5"),
    ("=ADDRESS(1,2)", "$B$1"),
    ("=ADDRESS(1,2,4)", "B1"),
    ("=OFFSET(A1,1,0)", "2"),
    ("=INDIRECT(\"A2\")", "2"),
    ("=INDIRECT(\"nope\")", "#REF!"),
    ("=CHOOSE(2,\"a\",\"b\",\"c\")", "b"),
    ("=CHOOSE(9,\"a\",\"b\")", "#NUM!"),
    ("=UNIQUE(D1:D4)", "apple"),
    ("=FILTER(A1:A5,A1:A5>3)", "4"),
    ("=SORT(A1:A5)", "1"),
    ("=TRANSPOSE(A1:A2)", "1"),
    ("=SEQUENCE(2,2)", "1"),
    ("=ARRAYFORMULA(A1:A3*2)", "2"),
    ("=SUM(ARRAYFORMULA(A1:A3*2))", "12"),
    // Coercion, operators and cell references
    ("=\"a\"&\"b\"", "ab"),
    ("=\"a\"&1", "a1"),
    ("=1&2", "12"),
    ("=TRUE&\"x\"", "TRUEx"),
    ("=1+\"2\"", "3"),
    ("=1+\"x\"", "#VALUE!"),
    ("=\"2\"*\"3\"", "6"),
    ("=1+TRUE", "2"),
    ("=TRUE+TRUE", "2"),
    ("=1+Z1", "1"),
    ("=Z1&\"x\"", "x"),
    ("=-\"3\"", "-3"),
    ("=50%", "0.5"),
    ("=10%*200", "20"),
    ("=1<2", "true"),
    ("=2<=2", "true"),
    ("=\"a\"=\"A\"", "true"),
    ("=\"a\"<\"b\"", "true"),
    ("=1=\"1\"", "false"),
    ("=TRUE=1", "false"),
    ("=1<>2", "true"),
    ("=A1+A2", "3"),
    ("=A1&D1", "1apple"),
    ("=A1:A2", "1"),
    ("=SUM(A1:A2,B1:B2)", "33"),
    ("=SUM(A1,B1)", "11"),
    ("=SUM(\"3\",4)", "7"),
    ("=SUM(D1:D4)", "0"),
    ("=SUM(E1:E3)", "0"),
    ("=SUM(TRUE,TRUE)", "2"),
    ("=AVERAGE(D1:D4)", "#DIV/0!"),
    ("=C1", "#DIV/0!"),
    ("=C1+1", "#DIV/0!"),
    ("=IFERROR(C1,\"e\")", "e"),
    ("=ISERROR(C1)", "true"),
    ("=N(C1)", "#DIV/0!"),
    ("=A1=1", "true"),
    ("=Z1=0", "true"),
    ("=Z1=\"\"", "true"),
    ("=LEN(Z1)", "0"),
    ("=1/C2", "0.5"),
    ("=H1", "45296"),
    ("=H2", "0.5"),
    ("=H3", "5"),
    ("=H4", "1000"),
    ("=H1+0", "45296"),
    ("=H2*2", "1"),
    ("=H3*2", "10"),
    ("=H4/2", "500"),
    // Regressions this table was written for
    ("=SUBTOTAL(9,A1:D1)", "#DIV/0!"),
    ("=SUBTOTAL(101,A1:A5)", "3"),
    ("=NOSUCHFN(\"a\")", "#NAME?"),
    ("=NOSUCHFN(1)", "#NAME?"),
    ("=MATCH(\"?pple\",D1:D4,0)", "1"),
    ("=MATCH(\"a~*\",D1:D4,0)", "#N/A"),
    ("=COUNTIF(D1:D4,\"?pple\")", "2"),
    ("=ISURL(\"https://example.com\")", "true"),
    ("=ISURL(\"http://a.b/c\")", "false"),
    ("=DOLLAR(1235.5,0)", "$1,236"),
    ("=FIXED(1234.65,1)", "1,234.7"),
    ("=TEXT(2.5,\"0\")", "3"),
    ("=TEXT(-2.5,\"0\")", "-3"),
    ("=TEXT(1234.5,\"#,##0\")", "1,235"),
    ("=STDEV(Z1:Z9)", "#DIV/0!"),
    ("=VAR(Z1:Z9)", "#DIV/0!"),
    ("=LARGE(Z1:Z9,1)", "#NUM!"),
    ("=GEOMEAN(Z1:Z9)", "#NUM!"),
    ("=HARMEAN(Z1:Z9)", "#NUM!"),
];

fn fixture() -> SpreadsheetWorkbook {
    let mut workbook = SpreadsheetWorkbook::empty("Golden");
    workbook.add_sheet_with_id("sheet-1", "Data");
    for (address, value) in FIXTURE {
        workbook
            .set_cell_in_sheet("sheet-1", address, (*value).to_string())
            .expect("fixture sheet exists");
    }
    workbook.evaluate();
    workbook
}

/// Evaluates `formula` in `Y1` of a copy of the fixture and returns the
/// cell's canonical computed text.
fn evaluate(base: &SpreadsheetWorkbook, formula: &str) -> String {
    let mut workbook = base.clone();
    workbook
        .set_cell_in_sheet("sheet-1", "Y1", formula.to_string())
        .expect("fixture sheet exists");
    workbook.evaluate();
    workbook.sheets[0]
        .cells
        .iter()
        .find(|cell| cell.address == "Y1")
        .map(|cell| cell.computed_value.clone())
        .unwrap_or_default()
}

fn check(rows: &[(&str, &str)]) {
    let base = fixture();
    let mut failures = Vec::new();
    for (formula, expected) in rows {
        let actual = evaluate(&base, formula);
        if actual != *expected {
            failures.push(format!(
                "  {formula}\n    expected {expected:?}\n    actual   {actual:?}"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} golden rows disagree:\n{}",
        failures.len(),
        rows.len(),
        failures.join("\n")
    );
}

#[test]
fn golden_formula_table() {
    check(GOLDEN);
}

#[test]
fn the_table_has_no_duplicate_formulas() {
    let mut seen = std::collections::BTreeSet::new();
    let duplicates: Vec<&str> = GOLDEN
        .iter()
        .filter(|(formula, _)| !seen.insert(*formula))
        .map(|(formula, _)| *formula)
        .collect();
    assert!(duplicates.is_empty(), "duplicate rows: {duplicates:?}");
}

// ---------------------------------------------------------------------
// The six wrong answers the table was written for. Each is already a row
// above; each also gets a named test, so a regression says which rule
// broke rather than only which line moved.
// ---------------------------------------------------------------------

/// `A1:D1` holds `1`, `2`, an error and text. COUNT counts numbers, and an
/// error cell is not a number — it is skipped, not propagated. A *direct*
/// error argument is still an error.
#[test]
fn count_skips_errors_inside_a_range_but_not_a_direct_one() {
    check(&[
        ("=COUNT(A1:D1)", "2"),
        ("=COUNT(1/0)", "#DIV/0!"),
        // Every other aggregate still propagates it.
        ("=SUM(A1:D1)", "#DIV/0!"),
        ("=AVERAGE(A1:D1)", "#DIV/0!"),
        // COUNTA counts the error cell: it is not blank.
        ("=COUNTA(A1:D1)", "4"),
    ]);
}

/// An empty range contributes no numbers. MIN/MAX/PRODUCT report the
/// identity 0 rather than #N/A or #VALUE!; AVERAGE stays #DIV/0! because
/// there the division by a zero count is the error.
#[test]
fn min_max_and_product_over_an_empty_range_are_zero() {
    check(&[
        ("=MAX(Z1:Z9)", "0"),
        ("=MIN(Z1:Z9)", "0"),
        ("=PRODUCT(Z1:Z9)", "0"),
        ("=SUM(Z1:Z9)", "0"),
        ("=COUNT(Z1:Z9)", "0"),
        ("=AVERAGE(Z1:Z9)", "#DIV/0!"),
        ("=MEDIAN(Z1:Z9)", "#NUM!"),
        // A range of only text holds no numbers either.
        ("=MAX(D1:D4)", "0"),
    ]);
}

/// ROUND's places argument is optional and defaults to 0, exactly as it
/// already did for ROUNDUP, ROUNDDOWN and TRUNC. Halves round away from
/// zero.
#[test]
fn round_takes_one_argument() {
    check(&[
        ("=ROUND(2.5)", "3"),
        ("=ROUND(-2.5)", "-3"),
        ("=ROUND(2.4)", "2"),
        ("=ROUND(2.345,2)", "2.35"),
        ("=ROUND(1234,-2)", "1200"),
        ("=ROUNDUP(2.1)", "3"),
        ("=ROUNDDOWN(2.9)", "2"),
        ("=TRUNC(2.789)", "2"),
    ]);
}

/// FLOOR rounds down to a multiple of |significance|. The only combination
/// that errors is a positive number with a negative significance — there is
/// no multiple of a negative significance at or below a positive number.
#[test]
fn floor_only_rejects_a_positive_number_with_a_negative_significance() {
    check(&[
        ("=FLOOR(-4.5)", "-5"),
        ("=FLOOR(-4.5,2)", "-6"),
        ("=FLOOR(-4.5,-2)", "-6"),
        ("=FLOOR(4.5)", "4"),
        ("=FLOOR(4.5,2)", "4"),
        ("=FLOOR(4.5,-2)", "#NUM!"),
        ("=FLOOR(4.5,0)", "#DIV/0!"),
        // CEILING is the mirror and was already right.
        ("=CEILING(-4.5)", "-4"),
        ("=CEILING(-4.5,-2)", "-6"),
        ("=CEILING(4.5,-2)", "#NUM!"),
    ]);
}

/// `G3` and `G6` are group subtotals inside `G1:G6`. A SUBTOTAL spanning
/// the column reports 1+2+4+5, not 1+2+3+4+5+9 — ignoring nested
/// SUBTOTALs is the whole reason the function exists.
#[test]
fn subtotal_ignores_nested_subtotals() {
    check(&[
        ("=SUBTOTAL(9,G1:G6)", "12"),
        ("=SUM(G1:G6)", "24"),
        // Group totals themselves are unaffected.
        ("=SUBTOTAL(9,G1:G2)", "3"),
        // Error handling follows the function SUBTOTAL delegates to.
        ("=SUBTOTAL(2,A1:D1)", "2"),
        ("=SUBTOTAL(3,A1:D1)", "4"),
        ("=SUBTOTAL(9,A1:D1)", "#DIV/0!"),
        ("=SUBTOTAL(12,A1:A5)", "#VALUE!"),
    ]);
}

/// `format!("{}", value as i64)` saturates in Rust, so every number past
/// 2^63 used to print as the same integer. The in-memory `f64` was always
/// right; this is the text the grid shows and CSV export writes.
#[test]
fn large_numbers_print_in_scientific_notation_instead_of_saturating() {
    check(&[
        ("=1E21", "1E+21"),
        ("=2^63", "9.223372036854776E+18"),
        ("=FACT(25)", "1.5511210043330986E+25"),
        // The boundary: plain digits while every digit is meaningful.
        ("=999999999999999", "999999999999999"),
        ("=1000000000000000", "1E+15"),
        ("=-2^63", "-9.223372036854776E+18"),
    ]);
}

/// An unknown function name is a name problem, whatever its arguments are.
/// Coercing the arguments first reported `#VALUE!` for a function that does
/// not exist.
#[test]
fn an_unknown_function_reports_name_not_value() {
    check(&[
        ("=NOSUCHFN(\"a\")", "#NAME?"),
        ("=NOSUCHFN(1)", "#NAME?"),
        ("=NOSUCHFN()", "#NAME?"),
    ]);
}

/// Exact-match lookup takes the same wildcards COUNTIF already accepted.
#[test]
fn match_accepts_wildcards_on_exact_match() {
    check(&[
        ("=MATCH(\"a*\",D1:D4,0)", "1"),
        ("=MATCH(\"?pple\",D1:D4,0)", "1"),
        ("=MATCH(\"*rry\",D1:D4,0)", "3"),
        // A literal star, escaped, matches nothing here.
        ("=MATCH(\"a~*\",D1:D4,0)", "#N/A"),
        // No wildcard in the key is still a plain exact match.
        ("=MATCH(\"apple\",D1:D4,0)", "1"),
        ("=COUNTIF(D1:D4,\"a*\")", "2"),
    ]);
}
