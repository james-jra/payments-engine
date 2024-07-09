use payments_engine::run_with_csv;

// Split a string by newline and sort lines based on first csv value
// Hacky way to compare CSV output that isn't deterministically ordered.
fn split_and_sort(input: String) -> String {
    let mut lines: Vec<_> = input.split('\n').collect();
    lines.sort_unstable_by(|a, b| {
        let aword = a.split_once(',').unwrap_or(("", "")).0;
        let bword = b.split_once(',').unwrap_or(("", "")).0;
        aword.cmp(bword)
    });
    lines.join("\n")
}

#[test]
fn deposit_and_withdraw_decimal() {
    let input = r"type, client, tx, amount
deposit,    1, 1, 10
deposit,    1, 2, 10
deposit,    2, 3, 10
deposit,    2, 4, 10
withdrawal, 1, 5, 5.5
withdrawal, 2, 6, 5.5
";
    let expected_output = r"client,available,held,total,locked
2,14.5,0,14.5,false
1,14.5,0,14.5,false
"
    .to_string();

    let mut output: Vec<u8> = vec![];
    let (rejects, fails) = run_with_csv(input.as_bytes(), &mut output).unwrap();

    let output = split_and_sort(String::from_utf8(output).unwrap());
    assert_eq!(output, split_and_sort(expected_output));
    assert_eq!(rejects.len(), 0);
    assert_eq!(fails.len(), 0);
}

#[test]
fn withdraw_reject_insufficient_funds() {
    let input = r"type, client, tx, amount
deposit,    1, 1, 10
deposit,    2, 2, 10
withdrawal, 1, 3, 5.5
withdrawal, 2, 4, 5.5
withdrawal, 2, 5, 5.5
";
    let expected_output = r"client,available,held,total,locked
1,4.5,0,4.5,false
2,4.5,0,4.5,false
"
    .to_string();

    let mut output: Vec<u8> = vec![];
    let (rejects, fails) = run_with_csv(input.as_bytes(), &mut output).unwrap();

    let output = split_and_sort(String::from_utf8(output).unwrap());
    assert_eq!(output, split_and_sort(expected_output));
    assert_eq!(rejects.len(), 1);
    assert_eq!(rejects[0].0, 5);
    assert_eq!(fails.len(), 0);
}

#[test]
fn dispute_held_funds() {
    let input = r"type, client, tx, amount
deposit,    1, 1, 10
deposit,    1, 2, 5
withdrawal, 1, 3, 2
dispute,    1, 2
";
    let expected_output = r"client,available,held,total,locked
1,8,5,13,false
"
    .to_string();

    let mut output: Vec<u8> = vec![];
    let (rejects, fails) = run_with_csv(input.as_bytes(), &mut output).unwrap();

    let output = split_and_sort(String::from_utf8(output).unwrap());
    assert_eq!(output, split_and_sort(expected_output));
    assert_eq!(rejects.len(), 0);
    assert_eq!(fails.len(), 0);
}

#[test]
fn dispute_resolve() {
    let input = r"type, client, tx, amount
deposit,    1, 1, 10
deposit,    1, 2, 5
withdrawal, 1, 3, 2
dispute,    1, 2
resolve,    1, 2
";
    let expected_output = r"client,available,held,total,locked
1,13,0,13,false
"
    .to_string();

    let mut output: Vec<u8> = vec![];
    let (rejects, fails) = run_with_csv(input.as_bytes(), &mut output).unwrap();

    let output = split_and_sort(String::from_utf8(output).unwrap());
    assert_eq!(output, split_and_sort(expected_output));
    assert_eq!(rejects.len(), 0);
    assert_eq!(fails.len(), 0);
}

#[test]
fn dispute_resolve_redispute_chargeback() {
    let input = r"type, client, tx, amount
deposit,    1, 1, 10
deposit,    1, 2, 5
withdrawal, 1, 3, 2
dispute,    1, 2
resolve,    1, 2
dispute,    1, 2
chargeback, 1, 2
";
    let expected_output = r"client,available,held,total,locked
1,8,0,8,true
"
    .to_string();

    let mut output: Vec<u8> = vec![];
    let (rejects, fails) = run_with_csv(input.as_bytes(), &mut output).unwrap();

    let output = split_and_sort(String::from_utf8(output).unwrap());
    assert_eq!(output, split_and_sort(expected_output));
    assert_eq!(rejects.len(), 0);
    assert_eq!(fails.len(), 0);
}

#[test]
fn chargeback_leads_to_overdrawn() {
    let input = r"type, client, tx, amount
deposit,    1, 1, 10
deposit,    1, 2, 5
withdrawal, 1, 3, 10
dispute,    1, 1
chargeback, 1, 1
";
    let expected_output = r"client,available,held,total,locked
1,0,0,-5,true
"
    .to_string();

    let mut output: Vec<u8> = vec![];
    let (rejects, fails) = run_with_csv(input.as_bytes(), &mut output).unwrap();

    let output = split_and_sort(String::from_utf8(output).unwrap());
    assert_eq!(output, split_and_sort(expected_output));
    assert_eq!(rejects.len(), 0);
    assert_eq!(fails.len(), 0);
}
