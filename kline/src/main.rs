use std::{collections::HashMap, fs, path::PathBuf};

fn main() {
    let mut sum = 0.0;
    for ((f1, f2), f3) in fs::read_dir("./v3_kline_2021_06_23")
        .unwrap()
        .zip(fs::read_dir("./v3_kline_2021_06_23").unwrap().skip(1))
        .zip(fs::read_dir("./v3_kline_2021_06_23").unwrap().skip(2))
    {
        let (f1, f2, f3) = (f1.unwrap(), f2.unwrap(), f3.unwrap());
        let prev = handle_raw_data(f1.path());
        let current = handle_raw_data(f2.path());
        let next = handle_raw_data(f3.path());
        for (name, (price, _)) in current {
            if let Some((prev_price, _)) = prev.get(&name) {
                if price / prev_price > 1.01 {
                    if let Some((next_price, next_quantity)) = next.get(&name) {
                        sum += (next_price - price) * next_quantity;
                    }
                }
            }
        }
    }
    println!("{}", &sum)
}

fn handle_raw_data(path: PathBuf) -> HashMap<String, (f64, f64)> {
    let mut h = HashMap::new();
    let s = fs::read_to_string(path).unwrap();
    for line in s.lines() {
        let line= line.split_whitespace().skip(2);
        let name: String = line.clone().take(3).collect();
        let price: f64 = line.clone().skip(4).take(1).collect::<String>().parse().unwrap();
        let quantity = line.skip(8).take(1).collect::<String>();//.parse().unwrap();
        let quantity: f64 = quantity.parse().unwrap();
        h.insert(name, (price, quantity));
    }
    h
}


