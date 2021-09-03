//
//  ViewController.swift
//  DylibExample
//
//  Created by Nathan Horrigan on 15/08/2021.
//

import UIKit

class ViewController: UIViewController {
    @IBOutlet weak var label: UILabel!
    
    override func viewDidLoad() {
        super.viewDidLoad()
        let sum = calculate_sum(1, 3)
        label.text = "The sum of 1 + 3 = \(sum)"
    }
}
